//! Authentication against the Harmont API.
use std::time::{Duration, Instant};

use harmont_cloud::{HarmontClient, HarmontError};
use hm_common::url_nonce::UrlNonce;
use hm_core::{app_ctx::AppCtx, config::ResolvedCloudConfig};
use secrecy::ExposeSecret as _;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tracing::{info, instrument, warn};
use url::Url;

/// How long to poll for the token before giving up.
const CLAIM_TIMEOUT: Duration = Duration::from_mins(3);

/// How long to wait between claim attempts.
const CLAIM_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Debug, Error)]
pub enum BrowserAuthError {
    #[error("failed to create listener: {_0}")]
    CouldNotCreateListener(std::io::Error),

    #[error("failed to deduce local address: {_0}")]
    CouldNotDeduceAddress(std::io::Error),
}

/// The code-path that opens the browser and serves the loopback redirect.
struct BrowserAuth;

impl BrowserAuth {
    /// Bind the loopback listener, open the browser to the login page, and
    /// spawn the task that serves the redirect.
    #[instrument]
    async fn open(app: Url, nonce: &UrlNonce) -> Result<(), BrowserAuthError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(BrowserAuthError::CouldNotCreateListener)?;
        let port = listener
            .local_addr()
            .map_err(BrowserAuthError::CouldNotDeduceAddress)?
            .port();

        let mut url = app;
        url.set_path("/cli-login");
        url.query_pairs_mut()
            .append_pair("port", &port.to_string())
            .append_pair("nonce", &nonce.base_64());

        info!("opening browser to {url}");
        if webbrowser::open(url.as_str()).is_err() {
            warn!("couldn't open a browser automatically. open this URL manually:\n  {url}");
        }

        tokio::spawn(Self::accept(listener));
        Ok(())
    }

    async fn accept(listener: TcpListener) {
        let (stream, _src_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                warn!(err = ?err, "failed to connect to the client. try connecting again.");
                return;
            }
        };

        let (reader, mut writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);

        let mut request_line = String::new();
        let _ = buf_reader.read_line(&mut request_line).await;

        let body = "<html><body>Login received. You can close this tab.</body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        writer.write_all(response.as_bytes()).await.ok();
        writer.shutdown().await.ok();
    }
}

/// A failure during the paste-in login flow.
#[derive(Debug, Error)]
pub enum PasteAuthError {
    /// The code could not be read from the terminal.
    #[error("failed to read the code: {0}")]
    Prompt(String),

    /// The code could not be redeemed for a token.
    #[error(transparent)]
    Redeem(#[from] HarmontError),
}

/// The code-path where the user pastes a code shown by the browser rather than
/// completing a loopback redirect.
#[derive(Debug)]
struct PasteTokenAuth;

impl PasteTokenAuth {
    /// Open the paste page, read a code, and redeem it for a token.
    ///
    /// # Errors
    ///
    /// [`PasteAuthError::Prompt`] when the terminal read fails;
    /// [`PasteAuthError::Redeem`] when the API rejects the code.
    async fn login(client: &HarmontClient, app: Url) -> Result<String, PasteAuthError> {
        let mut url = app;
        url.set_path("/cli-login");
        url.query_pairs_mut().append_pair("paste", "true");

        info!("open this URL in your browser, then paste the code:\n  {url}");

        let code = Self::read_code().await?;
        Ok(client.redeem_code(&code).await?)
    }

    /// Prompt for a login code, re-prompting until a non-empty code is entered.
    async fn read_code() -> Result<String, PasteAuthError> {
        tokio::task::spawn_blocking(|| {
            loop {
                let raw = dialoguer::Input::<String>::new()
                    .with_prompt("code")
                    .interact()
                    .map_err(|e| PasteAuthError::Prompt(e.to_string()))?;
                let code = raw.trim().to_string();
                if !code.is_empty() {
                    return Ok(code);
                }
            }
        })
        .await
        .map_err(|e| PasteAuthError::Prompt(e.to_string()))?
    }
}

/// A failure while polling for the browser-parked token.
#[derive(Debug, Error)]
pub enum ClaimError {
    /// The token was not parked within [`CLAIM_TIMEOUT`].
    #[error("timed out waiting for the browser to authorize this login")]
    TimedOut,
    /// The claim request itself failed.
    #[error(transparent)]
    Api(#[from] HarmontError),
}

/// Polls the cloud until the browser parks a token under our login nonce.
struct ClaimPoller<'client> {
    client: &'client HarmontClient,
    nonce: UrlNonce,
}

impl<'client> ClaimPoller<'client> {
    const fn new(client: &'client HarmontClient, nonce: UrlNonce) -> Self {
        Self { client, nonce }
    }

    /// Poll until the token is parked, or the polling window elapses.
    ///
    /// # Errors
    ///
    /// [`ClaimError::TimedOut`] when no token is parked within
    /// [`CLAIM_TIMEOUT`]; [`ClaimError::Api`] on any other request failure.
    async fn poll(&self) -> Result<String, ClaimError> {
        let nonce = self.nonce.base_64();
        let deadline = Instant::now() + CLAIM_TIMEOUT;
        loop {
            match self.client.claim_token(&nonce).await {
                Ok(token) => return Ok(token),
                Err(HarmontError::Api {
                    status: 400, code, ..
                }) if code == "cli_code_invalid" => {
                    if Instant::now() >= deadline {
                        return Err(ClaimError::TimedOut);
                    }
                    tokio::time::sleep(CLAIM_INTERVAL).await;
                }
                Err(err) => return Err(ClaimError::Api(err)),
            }
        }
    }
}

/// A failure during a login attempt.
#[derive(Debug, Error)]
pub enum LoginError {
    /// The environment has no browser to drive an interactive login.
    #[error("no browser is available for interactive login")]
    Unsupported,
    /// The browser flow could not be set up.
    #[error(transparent)]
    Browser(#[from] BrowserAuthError),
    /// The token could not be claimed after the browser flow.
    #[error(transparent)]
    Claim(#[from] ClaimError),
    /// The paste-in flow failed.
    #[error(transparent)]
    Paste(#[from] PasteAuthError),
}

/// A failure while reading the current user.
#[derive(Debug, Error)]
pub enum WhoamiError {
    /// No credentials are stored.
    #[error("not logged in — run `hm cloud auth login`")]
    NotLoggedIn,
    /// The user profile could not be read.
    #[error("could not read user profile: {0}")]
    Fetch(String),
}

#[derive(Debug)]
pub struct AuthProvider<'app, 'config> {
    app_ctx: &'app AppCtx,
    config: &'config ResolvedCloudConfig,
}

impl<'app, 'config> AuthProvider<'app, 'config> {
    /// Create a new authentication provider.
    #[must_use]
    pub const fn new(app_ctx: &'app AppCtx, config: &'config ResolvedCloudConfig) -> Self {
        Self { app_ctx, config }
    }

    /// Log in — browser-loopback when a GUI is available, otherwise the
    /// paste-in flow — persisting the token and confirming the signed-in user.
    ///
    /// # Errors
    ///
    /// [`LoginError::Unsupported`] when there is neither a browser nor an
    /// interactive terminal; [`LoginError::Browser`], [`LoginError::Claim`],
    /// or [`LoginError::Paste`] when the chosen flow fails.
    pub async fn try_login(&self) -> Result<(), LoginError> {
        let client = HarmontClient::anonymous(self.config.domain.api_url());
        let term = self.app_ctx.term();
        let token = if term.has_gui() && !term.is_ci() {
            self.login_browser(&client).await?
        } else if term.is_interactive() {
            self.login_paste(&client).await?
        } else {
            return Err(LoginError::Unsupported);
        };
        self.app_ctx.creds().set(&token).await;

        // Confirm by reading the user back — best-effort, the token is valid.
        match self.fetch_user(&token).await {
            Ok((name, email, _)) => info!("logged in as {name} ({email})"),
            Err(e) => warn!("logged in, but could not read user profile: {e}"),
        }
        Ok(())
    }

    /// Clear the stored credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential store cannot be cleared.
    pub async fn logout(&self) -> std::io::Result<()> {
        self.app_ctx.creds().clear().await?;
        info!("logged out");
        Ok(())
    }

    /// Print the user the stored token belongs to.
    ///
    /// # Errors
    ///
    /// [`WhoamiError::NotLoggedIn`] when no token is stored;
    /// [`WhoamiError::Fetch`] when the profile cannot be read.
    pub async fn whoami(&self) -> Result<(), WhoamiError> {
        let token = self
            .app_ctx
            .creds()
            .get()
            .await
            .ok_or(WhoamiError::NotLoggedIn)?;
        let (name, email, id) = self
            .fetch_user(token.expose_secret())
            .await
            .map_err(WhoamiError::Fetch)?;
        info!("{name} <{email}> (id {id})");
        Ok(())
    }

    /// The display name, email, and id of the user `token` authenticates as.
    async fn fetch_user(&self, token: &str) -> Result<(String, String, String), String> {
        let client = HarmontClient::with_base_url(token.to_owned(), self.config.domain.api_url());
        let me = client
            .raw()
            .get_current_user()
            .await
            .map_err(|e| e.to_string())?
            .into_inner();
        let name = me.name.clone().unwrap_or_else(|| me.email.clone());
        Ok((name, me.email.clone(), me.id.to_string()))
    }

    /// Open the browser, then claim the token the SPA parks under the nonce.
    async fn login_browser(&self, client: &HarmontClient) -> Result<String, LoginError> {
        let nonce = UrlNonce::random();
        BrowserAuth::open(self.config.domain.app(), &nonce).await?;

        // The token comes from polling; the spawned listener serves the browser
        // redirect concurrently, so a lost or slow redirect never blocks us —
        // the poll's retry loop is the wait.
        Ok(ClaimPoller::new(client, nonce).poll().await?)
    }

    /// Show the paste page and redeem the code the user enters.
    async fn login_paste(&self, client: &HarmontClient) -> Result<String, LoginError> {
        Ok(PasteTokenAuth::login(client, self.config.domain.app()).await?)
    }
}
