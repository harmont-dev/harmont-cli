//! `hm cloud secret set|list|rm` — manage org- and pipeline-scoped CI
//! secrets.
//!
//! Secret *values* are write-only: the API never returns them, so `list`
//! shows names and timestamps only. The CLI mirrors that — it never prints
//! a value back, even the one it just set.
//!
//! The published `harmont-cloud` SDK does not (yet) expose secret
//! operations, so these verbs make the authenticated HTTP calls directly,
//! reusing the same config/credential resolution the SDK-backed verbs use
//! (`hm_config` for the API base + active org, `hm_config::creds` for the
//! bearer token). No new auth stack — just a thin reqwest call for the
//! endpoints the generated client doesn't carry.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use crate::cli::SecretCommand;

/// Where the secret value comes from on `set`.
///
/// Exactly one source is allowed; the resolver rejects none and ambiguity.
#[derive(Debug, Clone)]
struct ValueSources<'a> {
    /// The positional `VALUE` argument, if given. `Some("-")` means stdin.
    positional: Option<&'a str>,
    /// The `--from-file <PATH>` argument, if given.
    from_file: Option<&'a Path>,
}

/// Build the REST path for the secrets collection, scoped to the org or to
/// a pipeline within it.
///
/// - org scope:      `/api/v0/organizations/{org}/secrets`
/// - pipeline scope: `/api/v0/organizations/{org}/pipelines/{pipeline}/secrets`
fn secrets_path(org: &str, pipeline: Option<&str>) -> String {
    match pipeline {
        Some(p) => format!("/api/v0/organizations/{org}/pipelines/{p}/secrets"),
        None => format!("/api/v0/organizations/{org}/secrets"),
    }
}

/// Append a URL-path-escaped secret name to a collection path.
fn secret_item_path(collection: &str, name: &str) -> String {
    format!("{collection}/{}", utf8_percent_encode(name, NON_ALPHANUMERIC))
}

/// Resolve the secret value from exactly one of: positional `VALUE`,
/// `--from-file`, or stdin (`VALUE` == `-`).
///
/// `read_stdin` is injected so the non-stdin branches are unit-testable; in
/// production it reads the real stdin.
///
/// Trailing newline handling: a single trailing `\n` (or `\r\n`) is trimmed
/// from file and stdin sources, since editors and `echo` append one and a
/// secret almost never wants it. The positional `VALUE` is taken verbatim.
///
/// # Errors
///
/// Returns an error if no source is given, if both a positional value and
/// `--from-file` are given (ambiguous), or if the file/stdin read fails.
fn resolve_value(
    sources: &ValueSources<'_>,
    read_stdin: impl FnOnce() -> Result<String>,
) -> Result<String> {
    match (sources.positional, sources.from_file) {
        (Some(_), Some(_)) => bail!(
            "ambiguous secret value: pass either VALUE or --from-file, not both\n  \u{2192} drop one source"
        ),
        (None, None) => bail!(
            "no secret value: pass VALUE, `-` to read stdin, or --from-file <PATH>\n  \u{2192} e.g. `hm cloud secret set TOKEN abc123` or `--from-file ./token.txt`"
        ),
        (Some("-"), None) => {
            let raw = read_stdin().context("reading secret value from stdin")?;
            Ok(trim_one_trailing_newline(&raw).to_string())
        }
        (Some(v), None) => Ok(v.to_string()),
        (None, Some(path)) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("reading secret value from {}", path.display()))?;
            Ok(trim_one_trailing_newline(&raw).to_string())
        }
    }
}

/// Strip a single trailing `\n` or `\r\n`, leaving any other whitespace
/// intact (a secret may legitimately contain interior or leading spaces).
fn trim_one_trailing_newline(s: &str) -> &str {
    s.strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(s)
}

/// Entry point dispatched from `cli::dispatch_command`.
pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: SecretCommand) -> Result<()> {
    // Share config/credential/org resolution — and its error strings — with
    // the SDK-backed verbs (`settings::client` / `ResolvedCtx::org`). These
    // verbs need the raw token + API base for direct reqwest calls, so they
    // use `raw_org_ctx` rather than building an SDK client.
    let (api, token, org) = crate::settings::raw_org_ctx()?;

    match cmd {
        SecretCommand::Set {
            name,
            value,
            from_file,
            pipeline,
        } => {
            set(
                &api,
                &token,
                &org,
                pipeline.as_deref(),
                &name,
                value.as_deref(),
                from_file.as_deref(),
            )
            .await
        }
        SecretCommand::List { pipeline } => {
            list(&api, &token, &org, pipeline.as_deref()).await
        }
        SecretCommand::Rm { name, pipeline } => {
            rm(&api, &token, &org, pipeline.as_deref(), &name).await
        }
    }
}

/// Build an authenticated reqwest client carrying the bearer token.
fn http_client(token: &str) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
        .context("API token contains characters invalid for an Authorization header")?;
    auth.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, auth);
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .context("building HTTP client")
}

/// Turn a non-success response into a readable error in the house shape
/// (`<what>\n  → <fix>`), honoring the status semantics from PRINCIPLES.md.
///
/// The cloud dispatcher (`cli::dispatch_command`) is anyhow-based and maps
/// every error to the generic runtime exit code, so this cannot itself
/// select the semantic auth/API exit codes (3/5). What it *can* do — and
/// does — is make the 401 message text match the existing not-logged-in
/// path (`settings::client` / `raw_org_ctx`) so an expired/absent token
/// reads consistently regardless of where it's detected, and add a `→` fix
/// line. See the module-level note and [`from_status`].
async fn into_api_error(resp: reqwest::Response) -> anyhow::Error {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    from_status(status, &body)
}

/// Pure status-to-error mapping, split out from [`into_api_error`] so it is
/// unit-testable without spinning up an HTTP server.
fn from_status(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    use reqwest::StatusCode;

    let server_message = || {
        let parsed: serde_json::Value =
            serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
        let obj = parsed.get("error").cloned().unwrap_or(parsed);
        obj.get("message")
            .and_then(|m| m.as_str())
            .map(ToOwned::to_owned)
    };

    match status {
        // Match the not-logged-in text used by `settings::raw_org_ctx` so an
        // expired/rejected token reads the same wherever it surfaces.
        StatusCode::UNAUTHORIZED => anyhow::anyhow!(
            "not logged in — your token was rejected\n  \u{2192} run `hm cloud login`"
        ),
        StatusCode::NOT_FOUND => anyhow::anyhow!(
            "secret or scope not found\n  \u{2192} check the secret name and `--pipeline` scope, or `hm cloud secret list`"
        ),
        _ => {
            let detail = server_message().unwrap_or_else(|| {
                if body.is_empty() {
                    status.as_str().to_owned()
                } else {
                    body.to_owned()
                }
            });
            anyhow::anyhow!("API error ({}): {detail}", status.as_u16())
        }
    }
}

async fn set(
    api: &str,
    token: &str,
    org: &str,
    pipeline: Option<&str>,
    name: &str,
    value: Option<&str>,
    from_file: Option<&Path>,
) -> Result<()> {
    let resolved = resolve_value(
        &ValueSources {
            positional: value,
            from_file,
        },
        read_stdin,
    )?;

    let url = format!("{api}{}", secrets_path(org, pipeline));
    let client = http_client(token)?;
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "name": name, "value": resolved }))
        .send()
        .await
        .context("sending request")?;
    if !resp.status().is_success() {
        return Err(into_api_error(resp).await);
    }
    let scope = pipeline.map_or_else(|| format!("org {org}"), |p| format!("pipeline {p}"));
    tracing::info!("set secret {name} ({scope})");
    Ok(())
}

async fn list(api: &str, token: &str, org: &str, pipeline: Option<&str>) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Secret {
        name: String,
        updated_at: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Listing {
        secrets: Vec<Secret>,
    }

    let url = format!("{api}{}", secrets_path(org, pipeline));
    let client = http_client(token)?;
    let resp = client.get(&url).send().await.context("sending request")?;
    if !resp.status().is_success() {
        return Err(into_api_error(resp).await);
    }
    let listing: Listing = resp.json().await.context("decoding response")?;
    if listing.secrets.is_empty() {
        tracing::info!("No secrets.");
        return Ok(());
    }
    for s in &listing.secrets {
        match &s.updated_at {
            Some(ts) => tracing::info!("{:<32} {ts}", s.name),
            None => tracing::info!("{}", s.name),
        }
    }
    Ok(())
}

async fn rm(api: &str, token: &str, org: &str, pipeline: Option<&str>, name: &str) -> Result<()> {
    let collection = secrets_path(org, pipeline);
    let url = format!("{api}{}", secret_item_path(&collection, name));
    let client = http_client(token)?;
    let resp = client.delete(&url).send().await.context("sending request")?;
    if !resp.status().is_success() {
        return Err(into_api_error(resp).await);
    }
    tracing::info!("removed secret {name}");
    Ok(())
}

/// Read all of stdin to a `String`.
fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn path_org_scope() {
        assert_eq!(
            secrets_path("acme", None),
            "/api/v0/organizations/acme/secrets"
        );
    }

    #[test]
    fn path_pipeline_scope() {
        assert_eq!(
            secrets_path("acme", Some("ci")),
            "/api/v0/organizations/acme/pipelines/ci/secrets"
        );
    }

    #[test]
    fn item_path_appends_encoded_name() {
        let coll = secrets_path("acme", None);
        assert_eq!(
            secret_item_path(&coll, "DEPLOY_TOKEN"),
            "/api/v0/organizations/acme/secrets/DEPLOY_TOKEN"
        );
    }

    #[test]
    fn item_path_escapes_funny_names() {
        let coll = secrets_path("acme", Some("ci"));
        assert_eq!(
            secret_item_path(&coll, "a/b c"),
            "/api/v0/organizations/acme/pipelines/ci/secrets/a%2Fb%20c"
        );
    }

    fn no_stdin() -> Result<String> {
        bail!("stdin must not be read in this branch")
    }

    #[test]
    fn resolve_positional_verbatim() {
        let sources = ValueSources {
            positional: Some("hunter2"),
            from_file: None,
        };
        assert_eq!(resolve_value(&sources, no_stdin).unwrap(), "hunter2");
    }

    #[test]
    fn resolve_positional_keeps_interior_spaces() {
        let sources = ValueSources {
            positional: Some("  spaced value  "),
            from_file: None,
        };
        // Positional is verbatim — no trimming at all.
        assert_eq!(
            resolve_value(&sources, no_stdin).unwrap(),
            "  spaced value  "
        );
    }

    #[test]
    fn resolve_from_file_trims_one_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.txt");
        std::fs::write(&path, "abc123\n").unwrap();
        let sources = ValueSources {
            positional: None,
            from_file: Some(&path),
        };
        assert_eq!(resolve_value(&sources, no_stdin).unwrap(), "abc123");
    }

    #[test]
    fn resolve_from_file_trims_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.txt");
        std::fs::write(&path, "abc123\r\n").unwrap();
        let sources = ValueSources {
            positional: None,
            from_file: Some(&path),
        };
        assert_eq!(resolve_value(&sources, no_stdin).unwrap(), "abc123");
    }

    #[test]
    fn resolve_from_file_keeps_interior_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.pem");
        std::fs::write(&path, "line1\nline2\n").unwrap();
        let sources = ValueSources {
            positional: None,
            from_file: Some(&path),
        };
        assert_eq!(resolve_value(&sources, no_stdin).unwrap(), "line1\nline2");
    }

    #[test]
    fn resolve_stdin_dash_trims_trailing_newline() {
        let sources = ValueSources {
            positional: Some("-"),
            from_file: None,
        };
        let got = resolve_value(&sources, || Ok("from-stdin\n".to_string())).unwrap();
        assert_eq!(got, "from-stdin");
    }

    #[test]
    fn resolve_errors_when_no_source() {
        let sources = ValueSources {
            positional: None,
            from_file: None,
        };
        let err = resolve_value(&sources, no_stdin).unwrap_err();
        assert!(err.to_string().contains("no secret value"));
    }

    #[test]
    fn resolve_errors_when_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.txt");
        std::fs::write(&path, "abc").unwrap();
        let sources = ValueSources {
            positional: Some("inline"),
            from_file: Some(&path),
        };
        let err = resolve_value(&sources, no_stdin).unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn resolve_missing_file_errors() {
        let sources = ValueSources {
            positional: None,
            from_file: Some(Path::new("/nonexistent/secret/path/xyz")),
        };
        assert!(resolve_value(&sources, no_stdin).is_err());
    }

    #[test]
    fn special_chars_are_encoded() {
        let coll = secrets_path("acme", None);
        assert_eq!(
            secret_item_path(&coll, "DEPLOY-TOKEN_v2.0~rc"),
            "/api/v0/organizations/acme/secrets/DEPLOY%2DTOKEN%5Fv2%2E0%7Erc"
        );
    }

    #[test]
    fn error_401_matches_not_logged_in_text() {
        let err = from_status(reqwest::StatusCode::UNAUTHORIZED, "");
        let msg = err.to_string();
        // Consistent with the `settings::raw_org_ctx` not-logged-in path.
        assert!(msg.contains("not logged in"), "got: {msg}");
        assert!(msg.contains("hm cloud login"), "got: {msg}");
    }

    #[test]
    fn error_404_is_not_found_with_hint() {
        let err = from_status(reqwest::StatusCode::NOT_FOUND, "");
        let msg = err.to_string();
        assert!(msg.contains("not found"), "got: {msg}");
        assert!(msg.contains('\u{2192}'), "expected a fix hint, got: {msg}");
    }

    #[test]
    fn error_other_surfaces_status_and_server_message() {
        let body = r#"{"error":{"message":"name already taken"}}"#;
        let err = from_status(reqwest::StatusCode::CONFLICT, body);
        let msg = err.to_string();
        assert!(msg.contains("409"), "got: {msg}");
        assert!(msg.contains("name already taken"), "got: {msg}");
    }
}
