//! `hm cloud auth whoami` — print the signed-in user.

use anyhow::Result;
use hm_core::app_ctx::AppCtx;

use crate::commands::cloud::settings;

pub(crate) async fn run(app: &AppCtx) -> Result<()> {
    let config = settings::auth_config(app);
    hm_cloud::auth::AuthProvider::new(app, &config)
        .whoami()
        .await
        .map_err(anyhow::Error::from)
}
