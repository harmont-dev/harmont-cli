//! `hm cloud auth logout` — clear the stored bearer token.

use anyhow::Result;
use hm_core::app_ctx::AppCtx;

use crate::commands::cloud::settings;

pub(crate) async fn run(app: &AppCtx) -> Result<()> {
    let config = settings::auth_config(app);
    hm_cloud::auth::AuthProvider::new(app, &config)
        .logout()
        .await
        .map_err(anyhow::Error::from)
}
