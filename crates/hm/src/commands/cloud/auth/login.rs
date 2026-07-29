//! `hm cloud auth login` — drive the shared login flow via
//! [`hm_cloud::auth::AuthProvider`].

use anyhow::Result;
use hm_core::app_ctx::AppCtx;

use crate::commands::cloud::settings;

pub(crate) async fn run(app: &AppCtx) -> Result<()> {
    let config = settings::auth_config(app);
    hm_cloud::auth::AuthProvider::new(app, &config)
        .try_login()
        .await
        .map_err(anyhow::Error::from)
}
