//! `hm cloud login` — browser-loopback or paste-in flow.

use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::api::types::{CliExchangeRequest, CliExchangeResponse, User};
use crate::config::Config;
use crate::creds;
use crate::http::Client;

pub(crate) async fn run(env: &BTreeMap<String, String>, paste: bool) -> Result<()> {
    let cfg = Config::from_env(env);
    let (verifier, challenge) = pkce_pair();

    if paste {
        login_paste(env, &cfg, &verifier, &challenge).await
    } else {
        login_loopback(env, &cfg, &verifier, &challenge).await
    }
}

async fn login_loopback(
    _env: &BTreeMap<String, String>,
    cfg: &Config,
    verifier: &str,
    challenge: &str,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    // Bind a local TCP listener on a random port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}/cb");
    let auth_url = format!(
        "{}/cli/login?challenge={}&redirect_uri={}",
        cfg.api_base,
        challenge,
        urlencoding(&redirect),
    );

    tracing::info!("opening browser to {auth_url}");
    if webbrowser::open(&auth_url).is_err() {
        tracing::warn!("couldn't auto-open the browser. Open this URL manually:\n  {auth_url}");
    }

    // Wait for a single connection with a 180-second timeout.
    let code = tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let (stream, _addr) = listener.accept().await?;
        let (reader, mut writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);
        let mut request_line = String::new();
        buf_reader.read_line(&mut request_line).await?;

        // Parse "GET /cb?code=XYZ HTTP/1.1"
        let mut code_value: Option<String> = None;
        if let Some(path) = request_line.split_whitespace().nth(1)
            && let Some(query) = path.split('?').nth(1)
        {
            for param in query.split('&') {
                if let Some(val) = param.strip_prefix("code=") {
                    code_value = Some(val.to_string());
                }
            }
        }

        // Send a minimal HTTP response.
        let body = if code_value.is_some() {
            "<html><body>Login successful. You can close this tab.</body></html>"
        } else {
            "<html><body>Login failed: no code received.</body></html>"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        writer.write_all(response.as_bytes()).await.ok();
        writer.shutdown().await.ok();

        Ok::<Option<String>, anyhow::Error>(code_value)
    })
    .await
    .map_err(|_| anyhow::anyhow!("browser callback did not arrive within 3 minutes"))??;

    let code = code.ok_or_else(|| anyhow::anyhow!("callback had no 'code' query parameter"))?;

    finalize(cfg, &code, verifier).await
}

async fn login_paste(
    env: &BTreeMap<String, String>,
    cfg: &Config,
    verifier: &str,
    challenge: &str,
) -> Result<()> {
    let auth_url = format!(
        "{}/cli/login?challenge={}&redirect_uri=urn:ietf:wg:oauth:2.0:oob",
        cfg.api_base, challenge,
    );
    tracing::info!("Open this URL in your browser, then paste the code:\n  {auth_url}");
    let _ = webbrowser::open(&auth_url);

    // Tests inject the code via `HARMONT_LOGIN_CODE` to avoid TTY.
    let code = if let Some(c) = env.get("HARMONT_LOGIN_CODE") {
        c.clone()
    } else {
        dialoguer::Input::<String>::new()
            .with_prompt("code")
            .interact()
            .map_err(|e| anyhow::anyhow!("failed to read code: {e}"))?
    };
    let code = code.trim().to_string();
    if code.is_empty() {
        bail!("no code pasted");
    }
    finalize(cfg, &code, verifier).await
}

async fn finalize(cfg: &Config, code: &str, verifier: &str) -> Result<()> {
    let client = Client::anonymous(cfg);
    let resp: CliExchangeResponse = client
        .post(
            "/cli/exchange",
            &CliExchangeRequest {
                code: code.to_string(),
                verifier: verifier.to_string(),
            },
        )
        .await?;
    creds::save_token(&cfg.api_base, &resp.token);

    let auth_client = Client::new(cfg, Some(resp.token));
    let me: User = auth_client.get("/auth/me").await?;
    tracing::info!(
        "logged in as {} ({})",
        me.display_name.clone().unwrap_or_else(|| me.email.clone()),
        me.email,
    );
    Ok(())
}

/// Generate a PKCE verifier + S256 challenge.
fn pkce_pair() -> (String, String) {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use sha2::{Digest, Sha256};

    let mut seed = [0u8; 32];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, b) in seed.iter_mut().enumerate() {
        *b = ((now >> (i % 16)) & 0xFF) as u8;
    }
    let verifier = URL_SAFE_NO_PAD.encode(seed);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
