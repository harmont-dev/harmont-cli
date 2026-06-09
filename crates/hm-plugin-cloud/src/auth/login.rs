//! `hm cloud login` — browser-loopback or paste-in flow, routed through the
//! SDK's anonymous auth endpoints.
//!
//! Two paths produce a bearer token:
//!
//! - **loopback** (default): the CLI generates a random nonce, binds a local
//!   listener, opens the SPA's `/cli-login` page with that nonce + the loopback
//!   port, then polls [`HarmontClient::claim_token`] until the SPA parks the
//!   token under the nonce (or the 60s window closes).
//! - **paste** (`--paste`): the SPA shows a short code; the user pastes it and
//!   the CLI exchanges it via [`HarmontClient::redeem_code`].

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Result, bail};
use harmont_cloud::{HarmontClient, HarmontError};

use crate::settings;

pub(crate) async fn run(env: &BTreeMap<String, String>, paste: bool) -> Result<()> {
    let (client, api) = settings::anon_client()?;
    let app = app_url(&api, env);

    let token = if paste {
        login_paste(env, &client, &app).await?
    } else {
        login_loopback(&client, &app).await?
    };

    hm_config::creds::set_cloud_token(&api, &token);

    // Confirm by reading back the authenticated user.
    let authed = HarmontClient::with_base_url(token, &api);
    match authed.raw().get_current_user().await {
        Ok(resp) => {
            let me = resp.into_inner();
            tracing::info!(
                "logged in as {} ({})",
                me.name.clone().unwrap_or_else(|| me.email.clone()),
                me.email,
            );
        }
        Err(e) => {
            tracing::warn!("logged in, but could not read user profile: {e}");
        }
    }
    Ok(())
}

async fn login_loopback(client: &HarmontClient, app: &str) -> Result<String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let nonce = random_nonce();

    // Bind a loopback listener so the SPA can signal "browser handed off".
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let auth_url = format!("{app}/cli-login?port={port}&nonce={nonce}");

    tracing::info!("opening browser to {auth_url}");
    if webbrowser::open(&auth_url).is_err() {
        tracing::warn!("couldn't auto-open the browser. Open this URL manually:\n  {auth_url}");
    }

    // Accept the SPA's redirect to /callback (best-effort UX: it lets the
    // browser tab show "done"). We don't depend on its query for the token —
    // the token is claimed by nonce below.
    let accept = async {
        if let Ok((stream, _addr)) = listener.accept().await {
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
    };
    // Give the browser up to 3 minutes to complete sign-in and redirect.
    let _ = tokio::time::timeout(Duration::from_secs(180), accept).await;

    // Poll the claim endpoint. The SPA parks the token under our nonce; until
    // then the endpoint returns 400 `cli_code_invalid`, which we retry.
    poll_claim(client, &nonce).await
}

/// Poll `claim_token` until the token is parked or the ~60s window elapses.
async fn poll_claim(client: &HarmontClient, nonce: &str) -> Result<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        match client.claim_token(nonce).await {
            Ok(token) => return Ok(token),
            Err(HarmontError::Api { status: 400, code, .. }) if code == "cli_code_invalid" => {
                if std::time::Instant::now() >= deadline {
                    bail!(
                        "timed out waiting for the browser to authorize this login (60s).\n  \
                         fix: re-run `hm cloud login`, or use `hm cloud login --paste`"
                    );
                }
                tokio::time::sleep(Duration::from_millis(750)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

async fn login_paste(
    env: &BTreeMap<String, String>,
    client: &HarmontClient,
    app: &str,
) -> Result<String> {
    let auth_url = format!("{app}/cli-login?paste=true");
    tracing::info!("Open this URL in your browser, then paste the code:\n  {auth_url}");
    let _ = webbrowser::open(&auth_url);

    // Tests inject the code via `HARMONT_LOGIN_CODE` to avoid a TTY.
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
    Ok(client.redeem_code(&code).await?)
}

/// Derive the SPA (app) base URL from the API base.
///
/// Priority: `HARMONT_APP_URL` env > heuristic mapping of `api.` → `app.` on
/// the API host > the API base itself (last-resort, dev fallback).
fn app_url(api: &str, env: &BTreeMap<String, String>) -> String {
    if let Some(u) = env.get("HARMONT_APP_URL").filter(|u| !u.is_empty()) {
        return u.trim_end_matches('/').to_string();
    }
    let api = api.trim_end_matches('/');
    if let Some(rest) = api.strip_prefix("https://api.") {
        return format!("https://app.{rest}");
    }
    if let Some(rest) = api.strip_prefix("http://api.") {
        return format!("http://app.{rest}");
    }
    api.to_string()
}

/// A URL-safe random nonce for the loopback handoff.
fn random_nonce() -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let id = uuid::Uuid::new_v4();
    URL_SAFE_NO_PAD.encode(id.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn app_url_maps_prod_api_to_app() {
        assert_eq!(
            app_url("https://api.harmont.dev", &env(&[])),
            "https://app.harmont.dev"
        );
    }

    #[test]
    fn app_url_env_override_wins() {
        assert_eq!(
            app_url(
                "https://api.harmont.dev",
                &env(&[("HARMONT_APP_URL", "http://localhost:5173/")])
            ),
            "http://localhost:5173"
        );
    }

    #[test]
    fn app_url_falls_back_to_api_for_unmapped_host() {
        assert_eq!(
            app_url("http://localhost:4000", &env(&[])),
            "http://localhost:4000"
        );
    }

    #[test]
    fn nonces_are_distinct() {
        assert_ne!(random_nonce(), random_nonce());
    }
}
