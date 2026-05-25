//! HTTP client using reqwest.

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};

use crate::config::Config;

pub(crate) struct Client {
    inner: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl Client {
    pub(crate) fn new(config: &Config, token: Option<String>) -> Self {
        Self {
            inner: reqwest::Client::new(),
            base: config.api_base.clone(),
            token,
        }
    }

    pub(crate) fn anonymous(config: &Config) -> Self {
        Self::new(config, None)
    }

    pub(crate) async fn get<O: DeserializeOwned>(&self, path: &str) -> Result<O> {
        self.send::<(), O>("GET", path, None).await
    }

    pub(crate) async fn post<I: Serialize, O: DeserializeOwned>(
        &self,
        path: &str,
        body: &I,
    ) -> Result<O> {
        self.send::<I, O>("POST", path, Some(body)).await
    }

    #[allow(dead_code)]
    pub(crate) async fn delete<O: DeserializeOwned>(&self, path: &str) -> Result<O> {
        self.send::<(), O>("DELETE", path, None).await
    }

    async fn send<I, O>(&self, method: &str, path: &str, body: Option<&I>) -> Result<O>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let url = format!("{}{path}", self.base);
        let method_parsed = method
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid HTTP method '{method}': {e}"))?;
        let mut req = self.inner.request(method_parsed, &url);
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req = req.header("Accept", "application/json");
        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").json(b);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("{method} {url}"))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(500).collect();
            bail!("{method} {url} → HTTP {status}: {snippet}");
        }
        let bytes = resp.bytes().await?;
        if bytes.is_empty() {
            return serde_json::from_slice(b"null").context("decode empty response");
        }
        serde_json::from_slice(&bytes).context("decode response JSON")
    }
}
