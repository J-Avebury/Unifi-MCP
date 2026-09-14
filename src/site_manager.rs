use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

const CONNECTOR_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_CONNECTOR_BODY_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct SiteManagerClient {
    client: Client,
    api_key: String,
    site_id: Option<String>,
    console_id: Option<String>,
}

impl SiteManagerClient {
    pub(crate) fn new(
        api_key: String,
        site_id: Option<String>,
        console_id: Option<String>,
        insecure_tls: bool,
    ) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(insecure_tls)
            .timeout(CONNECTOR_TIMEOUT)
            .build()
            .context("failed to build Site Manager HTTP client")?;
        Ok(Self {
            client,
            api_key,
            site_id,
            console_id,
        })
    }

    pub(crate) fn has_console_id(&self) -> bool {
        self.console_id.is_some()
    }

    pub(crate) async fn connector_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let console_id = self.console_id.as_deref().context(
            "UNIFI_SITE_MANAGER_CONSOLE_ID is required for the api.ui.com console connector",
        )?;
        let url = format!(
            "https://api.ui.com/v1/connector/consoles/{console_id}/{}",
            path.trim_start_matches('/')
        );
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json")
            .header("X-API-Key", &self.api_key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("api.ui.com console connector request failed")?;
        let status = response.status();
        let body = read_bounded_body(response, "api.ui.com console connector").await?;
        if !status.is_success() {
            bail!("api.ui.com console connector returned HTTP {status}");
        }
        serde_json::from_slice(&body).context("api.ui.com console connector returned non-JSON data")
    }

    pub(crate) async fn list_sites(&self) -> Result<Value> {
        self.get("https://api.ui.com/v1/sites").await
    }

    pub(crate) async fn isp_metrics(&self, metric_type: &str, duration: &str) -> Result<Value> {
        if !matches!(metric_type, "5m" | "1h") {
            bail!("Site Manager ISP metric type must be 5m or 1h");
        }
        let url = format!("https://api.ui.com/ea/isp-metrics/{metric_type}?duration={duration}");
        self.get(&url).await
    }

    pub(crate) async fn resolve_site_id(&self, requested: &str) -> Result<String> {
        if let Some(site_id) = self.site_id.as_deref() {
            return Ok(site_id.to_owned());
        }
        let payload = self.list_sites().await?;
        let rows = payload
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| payload.as_array())
            .context("Site Manager sites response did not contain a site list")?;
        let matching = rows
            .iter()
            .filter_map(|row| {
                let object = row.as_object()?;
                let id = object
                    .get("id")
                    .or_else(|| object.get("siteId"))?
                    .as_str()?;
                let match_name = [object.get("name"), object.get("internalReference")]
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|value| value.eq_ignore_ascii_case(requested));
                (match_name || id.eq_ignore_ascii_case(requested)).then(|| id.to_owned())
            })
            .collect::<Vec<_>>();
        if matching.len() == 1 {
            return Ok(matching[0].clone());
        }
        if rows.len() == 1 {
            return rows[0]
                .get("id")
                .or_else(|| rows[0].get("siteId"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .context("Site Manager site has no id");
        }
        bail!(
            "could not resolve Site Manager site '{requested}' without UNIFI_SITE_MANAGER_SITE_ID"
        )
    }

    async fn get(&self, url: &str) -> Result<Value> {
        let response = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Site Manager API request failed")?;
        let status = response.status();
        let body = read_bounded_body(response, "Site Manager API").await?;
        if status == StatusCode::NOT_FOUND {
            bail!("Site Manager endpoint '{url}' is not supported");
        }
        if !status.is_success() {
            bail!("Site Manager API returned HTTP {status}");
        }
        serde_json::from_slice(&body).context("Site Manager API returned non-JSON data")
    }
}

async fn read_bounded_body(response: reqwest::Response, label: &str) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CONNECTOR_BODY_BYTES)
    {
        bail!("{label} response exceeds the 10 MB body limit");
    }
    let body = response
        .bytes()
        .await
        .context("failed to read HTTP response")?;
    if body.len() as u64 > MAX_CONNECTOR_BODY_BYTES {
        bail!("{label} response exceeds the 10 MB body limit");
    }
    Ok(body.to_vec())
}
