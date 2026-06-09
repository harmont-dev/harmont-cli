//! `hm cloud billing balance|transactions|usage|topup|redeem`.

use std::collections::BTreeMap;

use anyhow::Result;
use harmont_cloud::HarmontClient;

use crate::cli::BillingCommand;
use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: BillingCommand) -> Result<()> {
    let (client, ctx) = settings::client()?;
    let org = ctx.org()?;

    match cmd {
        BillingCommand::Balance => balance(&client, &org).await,
        BillingCommand::Transactions { limit } => transactions(&client, &org, limit).await,
        BillingCommand::Usage { from, to } => {
            usage(&client, &org, from.as_deref(), to.as_deref()).await
        }
        BillingCommand::Topup {
            amount_usd,
            no_browser,
        } => topup(&client, &org, amount_usd, no_browser).await,
        BillingCommand::Redeem { code } => redeem(&client, &org, &code).await,
    }
}

async fn balance(client: &HarmontClient, org: &str) -> Result<()> {
    let b = client
        .raw()
        .get_billing_balance(org)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    let dollars = b.balance_cents as f64 / 100.0;
    tracing::info!("${dollars:.2}");
    Ok(())
}

async fn transactions(client: &HarmontClient, org: &str, limit: u32) -> Result<()> {
    let list = client
        .raw()
        .list_billing_transactions(org, None, Some(i64::from(limit)))
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    for t in &list.data {
        tracing::info!(
            "{}  {:>10} {:<18} {}",
            t.created_at.format("%Y-%m-%d %H:%M:%S"),
            t.amount_cents,
            t.source.to_string(),
            t.description.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn usage(
    client: &HarmontClient,
    org: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<()> {
    // The usage endpoint requires an explicit window. Default to the trailing
    // 30 days when the caller omits one (matching the dashboard default).
    let now = chrono::Utc::now();
    let default_from = (now - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let default_to = now.format("%Y-%m-%d").to_string();
    let from = from.unwrap_or(&default_from);
    let to = to.unwrap_or(&default_to);

    let u = client
        .raw()
        .get_billing_usage(org, from, to)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    tracing::info!(
        "{from} -> {to}: cpu {} s, mem {} GB·s, disk {} GB·s, ${:.2}",
        u.cpu_seconds,
        u.memory_gb_seconds,
        u.disk_gb_seconds,
        u.total_cents as f64 / 100.0
    );
    Ok(())
}

async fn topup(client: &HarmontClient, org: &str, amount_usd: u32, no_browser: bool) -> Result<()> {
    let r = client
        .raw()
        .create_checkout(
            org,
            &harmont_cloud_raw::types::CheckoutRequest {
                amount_cents: i64::from(amount_usd) * 100,
            },
        )
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    if no_browser {
        tracing::info!("{}", r.checkout_url);
    } else if webbrowser::open(&r.checkout_url).is_err() {
        tracing::warn!("couldn't open browser; URL:");
        tracing::warn!("{}", r.checkout_url);
    }
    Ok(())
}

async fn redeem(client: &HarmontClient, org: &str, code: &str) -> Result<()> {
    let r = client
        .raw()
        .redeem_coupon(
            org,
            &harmont_cloud_raw::types::RedeemCouponRequest {
                code: code.to_string(),
            },
        )
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    let dollars = r.credit_cents as f64 / 100.0;
    tracing::info!("credited ${dollars:.2}");
    Ok(())
}
