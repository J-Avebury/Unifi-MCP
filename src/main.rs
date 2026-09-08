use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, Url};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{env, fs};

const DEFAULT_BASE_URL: &str = "https://your-unifi-console.example";
const DEFAULT_SITE: &str = "default";
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const REDACTED: &str = "***REDACTED***";

const ALLOWED_NETWORK_ENDPOINTS: &[&str] = &[
    "stat/sta",
    "stat/alluser",
    "stat/rogueap",
    "list/wlanconf",
    "stat/device",
];

#[derive(Clone)]
struct UnifiMcp {
    unifi: UnifiClient,
}

#[derive(Clone)]
struct UnifiClient {
    client: Client,
    base_url: Url,
    site: String,
    api_key: String,
    redact_sensitive_fields: bool,
}

struct UnifiConfig {
    base_url: Url,
    site: String,
    api_key: String,
    insecure_tls: bool,
    redact_sensitive_fields: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ToolIndexArgs {
    /// Filter tools by category.
    category: Option<String>,
    /// Case-insensitive search over tool name, category, and description.
    search: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListDevicesArgs {
    /// Maximum number of devices to return.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListClientsArgs {
    /// Only return currently connected clients.
    online_only: Option<bool>,
    /// Case-insensitive match against name, hostname, IP, MAC, or vendor fields.
    query: Option<String>,
    /// Maximum number of clients to return.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RawNetworkEndpointArgs {
    /// UniFi Network API endpoint. Must be one of the server allowlist.
    endpoint: String,
    /// Request limit sent to UniFi.
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ToolCatalogEntry {
    name: &'static str,
    category: &'static str,
    description: &'static str,
    read_only: bool,
}

const TOOL_CATALOG: &[ToolCatalogEntry] = &[
    ToolCatalogEntry {
        name: "unifi_tool_index",
        category: "meta",
        description: "List the UniFi tools exposed by this Rust MCP server.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_network_status",
        category: "system",
        description: "Return a compact UniFi Network status summary.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_list_devices",
        category: "devices",
        description: "List UniFi Network devices for the configured site.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_list_clients",
        category: "clients",
        description: "List UniFi Network clients for the configured site.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_list_wlans",
        category: "wireless",
        description: "List configured UniFi Wi-Fi networks.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_list_rogue_aps",
        category: "devices",
        description: "List rogue access points detected by UniFi Network.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_list_talk_sites",
        category: "talk",
        description: "List UniFi Talk sites visible to the configured API key.",
        read_only: true,
    },
    ToolCatalogEntry {
        name: "unifi_raw_network_endpoint",
        category: "raw",
        description: "Call a read-only allowlisted UniFi Network API endpoint.",
        read_only: true,
    },
];

#[tool_router]
impl UnifiMcp {
    fn new(unifi: UnifiClient) -> Self {
        Self { unifi }
    }

    #[tool(
        name = "unifi_tool_index",
        description = "List the UniFi tools exposed by this Rust MCP server.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn tool_index(
        &self,
        Parameters(args): Parameters<ToolIndexArgs>,
    ) -> Result<CallToolResult, McpError> {
        let category = args
            .category
            .as_deref()
            .map(|value| value.to_ascii_lowercase());
        let search = args
            .search
            .as_deref()
            .map(|value| value.to_ascii_lowercase());
        let tools = TOOL_CATALOG
            .iter()
            .filter(|tool| {
                category
                    .as_deref()
                    .is_none_or(|category| tool.category == category)
            })
            .filter(|tool| {
                search.as_deref().is_none_or(|search| {
                    tool.name.to_ascii_lowercase().contains(search)
                        || tool.category.to_ascii_lowercase().contains(search)
                        || tool.description.to_ascii_lowercase().contains(search)
                })
            })
            .collect::<Vec<_>>();

        json_result(json!({
            "count": tools.len(),
            "tools": tools
        }))
    }

    #[tool(
        name = "unifi_network_status",
        description = "Return a compact UniFi Network status summary for devices, clients, Wi-Fi networks, and rogue APs.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn network_status(&self) -> Result<CallToolResult, McpError> {
        let devices = self.fetch_network("stat/device", Some(MAX_LIMIT)).await?;
        let clients = self.fetch_network("stat/sta", Some(MAX_LIMIT)).await?;
        let wlans = self.fetch_network("list/wlanconf", Some(MAX_LIMIT)).await?;
        let rogue_aps = self.fetch_network("stat/rogueap", Some(MAX_LIMIT)).await?;

        let device_rows = extract_rows(&devices);
        let client_rows = extract_rows(&clients);
        let wlan_rows = extract_rows(&wlans);
        let rogue_rows = extract_rows(&rogue_aps);

        json_result(json!({
            "site": &self.unifi.site,
            "counts": {
                "devices": device_rows.len(),
                "online_clients": client_rows.len(),
                "wifi_networks": wlan_rows.len(),
                "rogue_aps": rogue_rows.len()
            },
            "devices": compact_rows(device_rows, 50, &[
                "name", "model", "type", "mac", "ip", "version", "state", "adopted", "connected_at"
            ]),
            "wifi_networks": compact_rows(wlan_rows, 50, &[
                "name", "enabled", "security", "wlan_band", "schedule_enabled", "_id"
            ])
        }))
    }

    #[tool(
        name = "unifi_list_devices",
        description = "List UniFi Network devices for the configured site.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn list_devices(
        &self,
        Parameters(args): Parameters<ListDevicesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = bounded_limit(args.limit);
        let payload = self.fetch_network("stat/device", Some(limit)).await?;
        let rows = extract_rows(&payload);

        json_result(json!({
            "site": &self.unifi.site,
            "count": rows.len(),
            "devices": compact_rows(rows, limit, &[
                "name", "model", "type", "mac", "ip", "version", "state", "adopted",
                "inform_url", "uplink", "port_table"
            ])
        }))
    }

    #[tool(
        name = "unifi_list_clients",
        description = "List UniFi Network clients for the configured site.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn list_clients(
        &self,
        Parameters(args): Parameters<ListClientsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = bounded_limit(args.limit);
        let endpoint = if args.online_only.unwrap_or(true) {
            "stat/sta"
        } else {
            "stat/alluser"
        };
        let payload = self.fetch_network(endpoint, Some(MAX_LIMIT)).await?;
        let mut rows = extract_rows(&payload);

        if let Some(query) = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
        {
            rows.retain(|row| matches_client_query(row, query));
        }

        json_result(json!({
            "site": &self.unifi.site,
            "source_endpoint": endpoint,
            "count": rows.len(),
            "clients": compact_rows(rows, limit, &[
                "name", "hostname", "mac", "ip", "oui", "essid", "is_wired",
                "uptime", "last_seen", "signal", "rssi", "tx_rate", "rx_rate",
                "sw_mac", "ap_mac", "network"
            ])
        }))
    }

    #[tool(
        name = "unifi_list_wlans",
        description = "List configured UniFi Wi-Fi networks for the configured site.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn list_wlans(&self) -> Result<CallToolResult, McpError> {
        let payload = self.fetch_network("list/wlanconf", Some(MAX_LIMIT)).await?;
        let rows = extract_rows(&payload);

        json_result(json!({
            "site": &self.unifi.site,
            "count": rows.len(),
            "wifi_networks": compact_rows(rows, MAX_LIMIT, &[
                "name", "enabled", "security", "wlan_band", "usergroup_id",
                "schedule_enabled", "mac_filter_enabled", "_id"
            ])
        }))
    }

    #[tool(
        name = "unifi_list_rogue_aps",
        description = "List rogue access points detected by UniFi Network.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn list_rogue_aps(&self) -> Result<CallToolResult, McpError> {
        let payload = self.fetch_network("stat/rogueap", Some(MAX_LIMIT)).await?;
        let rows = extract_rows(&payload);

        json_result(json!({
            "site": &self.unifi.site,
            "count": rows.len(),
            "rogue_aps": compact_rows(rows, MAX_LIMIT, &[
                "essid", "bssid", "channel", "signal", "rssi", "oui",
                "last_seen", "first_seen", "is_adhoc"
            ])
        }))
    }

    #[tool(
        name = "unifi_list_talk_sites",
        description = "List UniFi Talk sites visible to the configured API key.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn list_talk_sites(&self) -> Result<CallToolResult, McpError> {
        let payload = self
            .unifi
            .talk_get("integration/v1/sites")
            .await
            .map_err(tool_error)?;

        json_result(payload)
    }

    #[tool(
        name = "unifi_raw_network_endpoint",
        description = "Call a read-only allowlisted UniFi Network API endpoint and return its JSON response.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn raw_network_endpoint(
        &self,
        Parameters(args): Parameters<RawNetworkEndpointArgs>,
    ) -> Result<CallToolResult, McpError> {
        let endpoint = args.endpoint.trim().trim_start_matches('/');
        if !ALLOWED_NETWORK_ENDPOINTS.contains(&endpoint) {
            return Err(McpError::invalid_params(
                "endpoint is not in the read-only allowlist",
                Some(json!({
                    "endpoint": args.endpoint,
                    "allowed": ALLOWED_NETWORK_ENDPOINTS
                })),
            ));
        }

        let payload = self
            .unifi
            .network_post(endpoint, Some(bounded_limit(args.limit)))
            .await
            .map_err(tool_error)?;

        json_result(payload)
    }

    async fn fetch_network(&self, endpoint: &str, limit: Option<usize>) -> Result<Value, McpError> {
        self.unifi
            .network_post(endpoint, limit)
            .await
            .map_err(tool_error)
    }
}

#[tool_handler(
    name = "unifi-mcp",
    version = "0.1.0",
    instructions = "Read-only local UniFi MCP server. Use unifi_tool_index to see the available tools."
)]
impl ServerHandler for UnifiMcp {}

impl UnifiConfig {
    fn from_env() -> Result<Self> {
        let base_url = resolve_base_url()?;
        let site = env_pair("UNIFI_NETWORK_SITE", "UNIFI_SITE")
            .unwrap_or_else(|| DEFAULT_SITE.to_string());
        let api_key = secret_env_pair("UNIFI_NETWORK_API_KEY", "UNIFI_API_KEY")?;
        let insecure_tls =
            if let Some(verify_ssl) = env_pair("UNIFI_NETWORK_VERIFY_SSL", "UNIFI_VERIFY_SSL") {
                !parse_bool_value("UNIFI_NETWORK_VERIFY_SSL/UNIFI_VERIFY_SSL", &verify_ssl)?
            } else {
                parse_bool_env_pair("UNIFI_NETWORK_INSECURE_TLS", "UNIFI_INSECURE_TLS", false)?
            };
        let redact_sensitive_fields = parse_bool_env_pair(
            "UNIFI_NETWORK_REDACT_SENSITIVE_FIELDS",
            "UNIFI_REDACT_SENSITIVE_FIELDS",
            true,
        )?;

        Ok(Self {
            base_url,
            site,
            api_key,
            insecure_tls,
            redact_sensitive_fields,
        })
    }
}

impl UnifiClient {
    fn new(config: UnifiConfig) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(config.insecure_tls)
            .build()
            .context("failed to build UniFi HTTP client")?;

        Ok(Self {
            client,
            base_url: config.base_url,
            site: config.site,
            api_key: config.api_key,
            redact_sensitive_fields: config.redact_sensitive_fields,
        })
    }

    async fn network_post(&self, endpoint: &str, limit: Option<usize>) -> Result<Value> {
        let url = self.network_url(endpoint)?;
        let response = self
            .client
            .post(url)
            .header("X-API-Key", &self.api_key)
            .json(&json!({ "_limit": bounded_limit(limit) }))
            .send()
            .await
            .context("UniFi Network request failed")?;

        self.parse_response(response.status(), response.text().await?)
            .await
    }

    async fn talk_get(&self, path: &str) -> Result<Value> {
        let url = self.proxy_url("talk", path)?;
        let response = self
            .client
            .get(url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("UniFi Talk request failed")?;

        self.parse_response(response.status(), response.text().await?)
            .await
    }

    async fn parse_response(&self, status: StatusCode, body: String) -> Result<Value> {
        let mut value = parse_json_response(status, body).await?;
        if self.redact_sensitive_fields {
            redact_sensitive(&mut value);
        }
        Ok(value)
    }

    fn network_url(&self, endpoint: &str) -> Result<Url> {
        let endpoint = endpoint.trim().trim_start_matches('/');
        let path = format!("network/api/s/{}/{}", self.site, endpoint);
        self.proxy_url_path(&path)
    }

    fn proxy_url(&self, app: &str, path: &str) -> Result<Url> {
        let path = format!("{}/{}", app.trim_matches('/'), path.trim_matches('/'));
        self.proxy_url_path(&path)
    }

    fn proxy_url_path(&self, path: &str) -> Result<Url> {
        let mut url = self.base_url.clone();
        let prefix = url.path().trim_end_matches('/');
        let path = path.trim_start_matches('/');
        let combined_path = if prefix.is_empty() || prefix == "/" {
            format!("/proxy/{path}")
        } else {
            format!("{prefix}/proxy/{path}")
        };
        url.set_path(&combined_path);
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }
}

async fn parse_json_response(status: StatusCode, body: String) -> Result<Value> {
    if !status.is_success() {
        bail!("UniFi API returned HTTP {status}: {body}");
    }

    serde_json::from_str(&body).with_context(|| format!("UniFi API returned non-JSON body: {body}"))
}

fn extract_rows(value: &Value) -> Vec<&Value> {
    if let Some(rows) = value.get("data").and_then(Value::as_array) {
        return rows.iter().collect();
    }

    if let Some(rows) = value.get("items").and_then(Value::as_array) {
        return rows.iter().collect();
    }

    if let Some(rows) = value.as_array() {
        return rows.iter().collect();
    }

    vec![value]
}

fn compact_rows(rows: Vec<&Value>, limit: usize, fields: &[&str]) -> Vec<Value> {
    rows.into_iter()
        .take(limit)
        .map(|row| {
            let Some(object) = row.as_object() else {
                return row.clone();
            };

            let mut compact = serde_json::Map::new();
            for field in fields {
                if let Some(value) = object.get(*field) {
                    compact.insert((*field).to_string(), value.clone());
                }
            }

            if compact.is_empty() {
                row.clone()
            } else {
                Value::Object(compact)
            }
        })
        .collect()
}

fn matches_client_query(row: &Value, query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    [
        "name", "hostname", "mac", "ip", "oui", "essid", "network", "sw_mac", "ap_mac",
    ]
    .iter()
    .filter_map(|field| row.get(*field))
    .filter_map(value_as_search_text)
    .any(|value| value.to_ascii_lowercase().contains(&query))
}

fn value_as_search_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn resolve_base_url() -> Result<Url> {
    if let Some(base_url) = env_pair("UNIFI_NETWORK_BASE_URL", "UNIFI_BASE_URL") {
        return Url::parse(&base_url).with_context(|| {
            format!("UNIFI_NETWORK_BASE_URL/UNIFI_BASE_URL is not a valid URL: {base_url}")
        });
    }

    if let Some(host) = env_pair("UNIFI_NETWORK_HOST", "UNIFI_HOST") {
        let base_url = if host.starts_with("http://") || host.starts_with("https://") {
            host
        } else if let Some(port) =
            env_pair("UNIFI_NETWORK_PORT", "UNIFI_PORT").filter(|port| port != "443")
        {
            format!("https://{host}:{port}")
        } else {
            format!("https://{host}")
        };

        return Url::parse(&base_url)
            .with_context(|| format!("UniFi host produced an invalid URL: {base_url}"));
    }

    Url::parse(DEFAULT_BASE_URL).context("default UniFi base URL is invalid")
}

fn env_pair(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| env::var(fallback).ok().filter(|value| !value.is_empty()))
}

fn secret_env_pair(primary: &str, fallback: &str) -> Result<String> {
    if let Some(value) = read_secret_env(primary)? {
        return Ok(value);
    }

    read_secret_env(fallback)?.with_context(|| format!("{primary} or {fallback} is required"))
}

fn read_secret_env(name: &str) -> Result<Option<String>> {
    let direct = env::var(name).ok().filter(|value| !value.is_empty());
    let file_var = format!("{name}_FILE");
    let file = env::var(&file_var).ok().filter(|value| !value.is_empty());

    match (direct, file) {
        (Some(_), Some(_)) => bail!("set only one of {name} or {file_var}"),
        (Some(value), None) => Ok(Some(value)),
        (None, Some(path)) => read_secret_file(&file_var, &path).map(Some),
        (None, None) => Ok(None),
    }
}

fn read_secret_file(name: &str, path: &str) -> Result<String> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read secret file {name}"))?;
    let value = contents.trim_end_matches(['\n', '\r']);

    if value.is_empty() {
        bail!("{name} points to an empty secret file");
    }
    if value.contains('\n') || value.contains('\r') {
        bail!("{name} points to a multi-line secret file");
    }
    if value.len() > 8192 {
        bail!("{name} points to an unexpectedly large secret file");
    }

    Ok(value.to_string())
}

fn parse_bool_env_pair(primary: &str, fallback: &str, default: bool) -> Result<bool> {
    match env_pair(primary, fallback) {
        Some(value) => parse_bool_value(&format!("{primary}/{fallback}"), &value),
        None => Ok(default),
    }
}

fn parse_bool_value(name: &str, value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{name} must be true or false"),
    }
}

fn redact_sensitive(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if is_sensitive_field(key) {
                    *child = Value::String(REDACTED.to_string());
                } else {
                    redact_sensitive(child);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                redact_sensitive(child);
            }
        }
        _ => {}
    }
}

fn is_sensitive_field(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("passphrase")
        || key.contains("preshared")
        || key.contains("private_key")
        || key.contains("privatekey")
        || key.contains("secret")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("token")
        || key.contains("mgmt_key")
        || key.contains("management_key")
        || key.contains("ssh_key")
        || key.contains("sshkey")
        || key.contains("snmp_community")
        || key.contains("auth_key")
        || key.contains("encryption_key")
        || key == "x_passphrase"
        || key == "x_password"
        || key == "x_ssh_password"
        || key == "wep_key"
        || key == "wpa_key"
        || key == "psk"
        || key == "pin"
        || key == "vpn_config"
        || key == "wireguard_config"
        || key == "ovpn"
}

fn json_result(value: Value) -> Result<CallToolResult, McpError> {
    match serde_json::to_string_pretty(&value) {
        Ok(text) => Ok(CallToolResult::success(vec![ContentBlock::text(text)])),
        Err(err) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
            "failed to serialize UniFi response: {err}"
        ))])),
    }
}

fn tool_error(err: anyhow::Error) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = UnifiConfig::from_env()?;
    let unifi = UnifiClient::new(config)?;
    let service = UnifiMcp::new(unifi).serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_nested_sensitive_fields() {
        let mut payload = json!({
            "data": [{
                "name": "Guest WiFi",
                "x_passphrase": "secret-passphrase",
                "vpn": {
                    "private_key": "secret-private-key",
                    "public_key": "safe-public-key"
                }
            }]
        });

        redact_sensitive(&mut payload);

        assert_eq!(payload["data"][0]["name"], "Guest WiFi");
        assert_eq!(payload["data"][0]["x_passphrase"], REDACTED);
        assert_eq!(payload["data"][0]["vpn"]["private_key"], REDACTED);
        assert_eq!(payload["data"][0]["vpn"]["public_key"], "safe-public-key");
    }

    #[test]
    fn bounds_tool_limits() {
        assert_eq!(bounded_limit(None), DEFAULT_LIMIT);
        assert_eq!(bounded_limit(Some(0)), 1);
        assert_eq!(bounded_limit(Some(MAX_LIMIT + 1)), MAX_LIMIT);
    }
}
