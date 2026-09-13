use anyhow::{Context, Result, bail};
use reqwest::{Client, Method, StatusCode, Url};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

use crate::{
    UnifiClient, UnifiConfig, integration_site_id_from_payload, parse_json_response,
    redact_sensitive,
};

impl UnifiClient {
    pub(crate) fn new(config: UnifiConfig) -> Result<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .danger_accept_invalid_certs(config.insecure_tls)
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build UniFi HTTP client")?;
        Ok(Self {
            client,
            base_url: config.base_url,
            site: config.site,
            api_key: config.api_key,
            username: config.username,
            password: config.password,
            authenticated: Arc::new(Mutex::new(false)),
            integration_site_id: Arc::new(Mutex::new(None)),
            redact_sensitive_fields: config.redact_sensitive_fields,
        })
    }
    pub(crate) async fn ensure_login(&self) -> Result<()> {
        if self.api_key.is_some() {
            return Ok(());
        }
        let mut authenticated = self.authenticated.lock().await;
        if *authenticated {
            return Ok(());
        }
        let username = self
            .username
            .as_deref()
            .context("local username is not configured")?;
        let password = self
            .password
            .as_deref()
            .context("local password is not configured")?;
        let body = json!({"username":username,"password":password,"remember":true});
        let mut last_status = None;
        for path in ["api/auth/login", "api/login"] {
            let response = self
                .client
                .post(self.root_url(path)?)
                .json(&body)
                .send()
                .await
                .context("UniFi login request failed")?;
            last_status = Some(response.status());
            if response.status().is_success() {
                *authenticated = true;
                return Ok(());
            }
            if response.status() != StatusCode::NOT_FOUND {
                break;
            }
        }
        bail!(
            "UniFi local login failed with HTTP {}",
            last_status.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        )
    }
    pub(crate) async fn network_request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.network_request_with_redaction(method, endpoint, body, true)
            .await
    }

    pub(crate) async fn network_request_unredacted(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.network_request_with_redaction(method, endpoint, body, false)
            .await
    }

    pub(crate) async fn network_request_with_redaction(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
        redact_response: bool,
    ) -> Result<Value> {
        self.ensure_login().await?;
        let url = self.network_url(endpoint)?;
        let mut delay = 100;
        for attempt in 0..3 {
            let mut request = self
                .client
                .request(method.clone(), url.clone())
                .header("Accept", "application/json");
            if let Some(key) = &self.api_key {
                request = request.header("X-API-Key", key);
            }
            if let Some(body) = &body {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .context("UniFi Network request failed")?;
            let status = response.status();
            let text = response.text().await?;
            if matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504) && attempt < 2 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                delay *= 2;
                continue;
            }
            if status == StatusCode::NOT_FOUND {
                bail!(
                    "Legacy UniFi Network endpoint '{endpoint}' is not supported by this controller version or enabled feature set"
                );
            }
            let mut value = parse_json_response(status, text)?;
            if redact_response && self.redact_sensitive_fields {
                redact_sensitive(&mut value);
            }
            return Ok(value);
        }
        unreachable!()
    }
    pub(crate) async fn integration_request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.integration_request_with_query(method, endpoint, body, &[])
            .await
    }
    pub(crate) async fn integration_request_with_query(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
        query: &[(&str, String)],
    ) -> Result<Value> {
        if self.api_key.is_none() {
            bail!(
                "The UniFi Network Integration API requires UNIFI_NETWORK_API_KEY or UNIFI_API_KEY"
            );
        }
        let site_id = self.integration_site().await?;
        let mut url = self.integration_url(&site_id, endpoint)?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json");
        if let Some(api_key) = &self.api_key {
            request = request.header("X-API-Key", api_key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("UniFi Network Integration API request failed")?;
        let status = response.status();
        let text = response.text().await?;
        if status == StatusCode::NOT_FOUND {
            bail!(
                "Integration API endpoint '{endpoint}' is not supported by this controller version or enabled feature set"
            );
        }
        let mut value = parse_json_response(status, text)?;
        if self.redact_sensitive_fields {
            redact_sensitive(&mut value);
        }
        Ok(value)
    }
    pub(crate) async fn integration_global_request(
        &self,
        method: Method,
        endpoint: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Value> {
        let api_key = self.api_key.as_deref().context(
            "The UniFi Network Integration API requires UNIFI_NETWORK_API_KEY or UNIFI_API_KEY",
        )?;
        let mut url = self.proxy_url_path(&format!(
            "network/integration/{}",
            endpoint.trim().trim_start_matches('/')
        ))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string())
            .append_pair("offset", &offset.to_string());
        let response = self
            .client
            .request(method, url)
            .header("Accept", "application/json")
            .header("X-API-Key", api_key)
            .send()
            .await
            .context("UniFi Network Integration API request failed")?;
        let status = response.status();
        let text = response.text().await?;
        if status == StatusCode::NOT_FOUND {
            bail!(
                "Integration API endpoint '{endpoint}' is not supported by this controller version or enabled feature set"
            );
        }
        let mut value = parse_json_response(status, text)?;
        if self.redact_sensitive_fields {
            redact_sensitive(&mut value);
        }
        Ok(value)
    }
    pub(crate) async fn integration_site(&self) -> Result<String> {
        let mut cached = self.integration_site_id.lock().await;
        if let Some(site_id) = cached.as_ref() {
            return Ok(site_id.clone());
        }
        let api_key = self
            .api_key
            .as_deref()
            .context("Integration API key is not configured")?;
        let url = self.proxy_url_path("network/integration/v1/sites")?;
        let response = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .header("X-API-Key", api_key)
            .send()
            .await
            .context("UniFi Network Integration site discovery failed")?;
        let status = response.status();
        let body = response.text().await?;
        let payload = parse_json_response(status, body)?;
        let site_id = integration_site_id_from_payload(&payload, &self.site)?;
        *cached = Some(site_id.clone());
        Ok(site_id)
    }
    fn network_url(&self, endpoint: &str) -> Result<Url> {
        self.proxy_url_path(&format!(
            "network/api/s/{}/{}",
            self.site,
            endpoint.trim().trim_start_matches('/')
        ))
    }
    fn integration_url(&self, site_id: &str, endpoint: &str) -> Result<Url> {
        self.proxy_url_path(&format!(
            "network/integration/v1/sites/{site_id}/{}",
            endpoint.trim().trim_start_matches('/')
        ))
    }
    fn root_url(&self, path: &str) -> Result<Url> {
        let mut url = self.base_url.clone();
        let prefix = url.path().trim_end_matches('/');
        url.set_path(&format!("{prefix}/{}", path.trim_start_matches('/')));
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }
    fn proxy_url_path(&self, path: &str) -> Result<Url> {
        let mut url = self.base_url.clone();
        let prefix = url.path().trim_end_matches('/');
        url.set_path(&if prefix.is_empty() || prefix == "/" {
            format!("/proxy/{}", path.trim_start_matches('/'))
        } else {
            format!("{prefix}/proxy/{}", path.trim_start_matches('/'))
        });
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }
}
