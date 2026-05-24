//! `hm cloud billing balance|transactions|usage|topup|redeem`.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::types::{
    Balance, RedeemRequest, RedeemResponse, TopupRequest, TopupResponse, TransactionList,
    UsageWindow,
};
use crate::cli::BillingCommand;
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

pub(crate) async fn run(env: &BTreeMap<String, String>, cmd: BillingCommand) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));
    let org = active_org()?;

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

async fn balance(client: &Client, org: &str) -> Result<()> {
    let b: Balance = client
        .get(&format!("/organizations/{org}/billing/balance"))
        .await?;
    let dollars = b.credits_usd_cents as f64 / 100.0;
    tracing::info!("${dollars:.2}");
    Ok(())
}

async fn transactions(client: &Client, org: &str, limit: u32) -> Result<()> {
    let list: TransactionList = client
        .get(&format!(
            "/organizations/{org}/billing/transactions?limit={limit}"
        ))
        .await?;
    for t in &list.data {
        tracing::info!(
            "{}  {:>10} {:<14} {}",
            t.at.format("%Y-%m-%d %H:%M:%S"),
            t.amount_cents,
            t.kind,
            t.memo.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn usage(client: &Client, org: &str, from: Option<&str>, to: Option<&str>) -> Result<()> {
    let mut q = vec![];
    if let Some(f) = from {
        q.push(format!("from={f}"));
    }
    if let Some(t) = to {
        q.push(format!("to={t}"));
    }
    let qs = if q.is_empty() {
        String::new()
    } else {
        format!("?{}", q.join("&"))
    };
    let u: UsageWindow = client
        .get(&format!("/organizations/{org}/billing/usage{qs}"))
        .await?;
    tracing::info!(
        "{} -> {}: {:.2} min, ${:.2}",
        u.from.format("%Y-%m-%d"),
        u.to.format("%Y-%m-%d"),
        u.minutes_used,
        u.cents_used as f64 / 100.0
    );
    Ok(())
}

async fn topup(client: &Client, org: &str, amount_usd: u32, no_browser: bool) -> Result<()> {
    let r: TopupResponse = client
        .post(
            &format!("/organizations/{org}/billing/topup"),
            &TopupRequest {
                org_slug: org.to_string(),
                amount_cents: i64::from(amount_usd) * 100,
            },
        )
        .await?;
    if no_browser {
        tracing::info!("{}", r.checkout_url);
    } else if webbrowser::open(&r.checkout_url).is_err() {
        tracing::warn!("couldn't open browser; URL:");
        tracing::warn!("{}", r.checkout_url);
    }
    Ok(())
}

async fn redeem(client: &Client, org: &str, code: &str) -> Result<()> {
    let r: RedeemResponse = client
        .post(
            &format!("/organizations/{org}/billing/redeem"),
            &RedeemRequest {
                org_slug: org.to_string(),
                code: code.to_string(),
            },
        )
        .await?;
    let dollars = r.credited_cents as f64 / 100.0;
    tracing::info!("credited ${dollars:.2}");
    Ok(())
}

fn active_org() -> Result<String> {
    CloudState::load()
        .active_org
        .ok_or_else(|| anyhow::anyhow!("no active organization; run `hm cloud org switch <slug>`"))
}
