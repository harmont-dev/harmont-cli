//! `hm cloud auth login` — drive the shared browser/paste login flow via
//! [`hm_cloud::auth::AuthProvider`], then confirm by reading the user back.

use anyhow::Result;
use harmont_cloud::HarmontClient;
use hm_core::app_ctx::AppCtx;
use hm_core::config::ResolvedCloudConfig;

use crate::commands::cloud::settings;

pub(crate) async fn run(app: &AppCtx) -> Result<()> {
    let (client, domain) = settings::anon_client(app);
    let api = domain.api_url();
    let config = ResolvedCloudConfig {
        domain,
        org: None,
        repo: None,
        default_pipeline: None,
    };

    let token = hm_cloud::auth::AuthProvider::new(app, &client, &config)
        .try_login()
        .await?;

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
        Err(e) => tracing::warn!("logged in, but could not read user profile: {e}"),
    }
    Ok(())
}
