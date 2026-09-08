use anyhow::{Context, Result, bail};
use reqwest::{Client, Method, StatusCode, Url};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, JsonObject, ListToolsResult,
        PaginatedRequestParams, ResultType, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::{Map, Value, json};
use std::{borrow::Cow, env, fs, sync::Arc, time::Duration};
use tokio::sync::Mutex;

const DEFAULT_BASE_URL: &str = "https://your-unifi-console.example";
const DEFAULT_SITE: &str = "default";
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const REDACTED: &str = "***REDACTED***";

macro_rules! spec {
    ($name:expr, $title:expr, $category:expr, $description:expr, $kind:expr) => {
        ToolSpec {
            name: $name,
            title: $title,
            category: $category,
            description: $description,
            kind: $kind,
        }
    };
}
macro_rules! list {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $key:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the configured UniFi Network site."),
            ToolKind::List {
                endpoint: $endpoint,
                output_key: $key
            }
        )
    };
}
macro_rules! detail {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $arg:expr, $fields:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Return ",
                $title,
                " from the configured UniFi Network site."
            ),
            ToolKind::Detail {
                endpoint: $endpoint,
                id_arg: $arg,
                id_fields: $fields
            }
        )
    };
}

#[derive(Clone)]
struct UnifiMcp {
    unifi: UnifiClient,
}

#[derive(Clone)]
struct UnifiClient {
    client: Client,
    base_url: Url,
    site: String,
    api_key: Option<String>,
    username: Option<String>,
    password: Option<String>,
    authenticated: Arc<Mutex<bool>>,
    redact_sensitive_fields: bool,
}

struct UnifiConfig {
    base_url: Url,
    site: String,
    api_key: Option<String>,
    username: Option<String>,
    password: Option<String>,
    insecure_tls: bool,
    redact_sensitive_fields: bool,
}

#[derive(Clone, Copy)]
enum ToolKind {
    Index,
    Execute,
    Batch,
    Dashboard,
    List {
        endpoint: &'static str,
        output_key: &'static str,
    },
    Detail {
        endpoint: &'static str,
        id_arg: &'static str,
        id_fields: &'static [&'static str],
    },
    LookupIp,
    Raw,
}

#[derive(Clone, Copy)]
struct ToolSpec {
    name: &'static str,
    title: &'static str,
    category: &'static str,
    description: &'static str,
    kind: ToolKind,
}

const DEVICE_IDS: &[&str] = &["_id", "id", "mac"];
const CLIENT_IDS: &[&str] = &["_id", "id", "mac"];
const CONFIG_IDS: &[&str] = &["_id", "id"];

const TOOLS: &[ToolSpec] = &[
    spec!(
        "unifi_tool_index",
        "Tool Index",
        "meta",
        "Search and filter the UniFi Network tool catalogue.",
        ToolKind::Index
    ),
    spec!(
        "unifi_execute",
        "Execute Tool",
        "meta",
        "Execute a discovered UniFi Network tool by name.",
        ToolKind::Execute
    ),
    spec!(
        "unifi_batch",
        "Batch Tools",
        "meta",
        "Execute up to 20 read-only UniFi Network tool calls in sequence.",
        ToolKind::Batch
    ),
    spec!(
        "unifi_get_dashboard",
        "Dashboard",
        "system",
        "Return a compact Network dashboard with health, devices, clients, WLANs, alarms, and events.",
        ToolKind::Dashboard
    ),
    list!(
        "unifi_get_network_health",
        "Network Health",
        "system",
        "stat/health",
        "health"
    ),
    list!(
        "unifi_list_devices",
        "List Devices",
        "devices",
        "stat/device",
        "devices"
    ),
    detail!(
        "unifi_get_device_details",
        "Device Details",
        "devices",
        "stat/device",
        "device_id",
        DEVICE_IDS
    ),
    list!(
        "unifi_get_device_stats",
        "Device Statistics",
        "devices",
        "stat/device",
        "devices"
    ),
    list!(
        "unifi_list_clients",
        "List Clients",
        "clients",
        "stat/sta",
        "clients"
    ),
    detail!(
        "unifi_get_client_details",
        "Client Details",
        "clients",
        "stat/alluser",
        "client_id",
        CLIENT_IDS
    ),
    spec!(
        "unifi_lookup_by_ip",
        "Lookup by IP",
        "clients",
        "Find a client or UniFi device by IP address.",
        ToolKind::LookupIp
    ),
    list!(
        "unifi_list_blocked_clients",
        "Blocked Clients",
        "clients",
        "stat/alluser",
        "blocked_clients"
    ),
    list!(
        "unifi_list_wlans",
        "List WLANs",
        "wireless",
        "list/wlanconf",
        "wlans"
    ),
    detail!(
        "unifi_get_wlan_details",
        "WLAN Details",
        "wireless",
        "list/wlanconf",
        "wlan_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_networks",
        "List Networks",
        "networks",
        "rest/networkconf",
        "networks"
    ),
    detail!(
        "unifi_get_network_details",
        "Network Details",
        "networks",
        "rest/networkconf",
        "network_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_events",
        "List Events",
        "events",
        "stat/event",
        "events"
    ),
    list!(
        "unifi_recent_events",
        "Recent Events",
        "events",
        "stat/event",
        "events"
    ),
    list!(
        "unifi_list_alarms",
        "List Alarms",
        "events",
        "stat/alarm",
        "alarms"
    ),
    list!(
        "unifi_get_alerts",
        "Alerts",
        "events",
        "stat/alarm",
        "alerts"
    ),
    list!(
        "unifi_get_anomalies",
        "Anomalies",
        "events",
        "stat/anomalies",
        "anomalies"
    ),
    list!(
        "unifi_list_rogue_aps",
        "Rogue APs",
        "wireless",
        "stat/rogueap",
        "rogue_aps"
    ),
    list!(
        "unifi_list_port_forwards",
        "Port Forwards",
        "routing",
        "rest/portforward",
        "port_forwards"
    ),
    detail!(
        "unifi_get_port_forward",
        "Port Forward Details",
        "routing",
        "rest/portforward",
        "port_forward_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_routes",
        "Static Routes",
        "routing",
        "rest/routing",
        "routes"
    ),
    detail!(
        "unifi_get_route_details",
        "Route Details",
        "routing",
        "rest/routing",
        "route_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_active_routes",
        "Active Routes",
        "routing",
        "stat/routing",
        "routes"
    ),
    list!(
        "unifi_list_usergroups",
        "User Groups",
        "clients",
        "list/usergroup",
        "usergroups"
    ),
    detail!(
        "unifi_get_usergroup_details",
        "User Group Details",
        "clients",
        "list/usergroup",
        "usergroup_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_firewall_groups",
        "Firewall Groups",
        "firewall",
        "rest/firewallgroup",
        "firewall_groups"
    ),
    detail!(
        "unifi_get_firewall_group_details",
        "Firewall Group Details",
        "firewall",
        "rest/firewallgroup",
        "firewall_group_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_legacy_firewall_rules",
        "Legacy Firewall Rules",
        "firewall",
        "rest/firewallrule",
        "firewall_rules"
    ),
    list!(
        "unifi_list_port_profiles",
        "Port Profiles",
        "switch",
        "rest/portconf",
        "port_profiles"
    ),
    detail!(
        "unifi_get_port_profile_details",
        "Port Profile Details",
        "switch",
        "rest/portconf",
        "port_profile_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_get_network_stats",
        "Network Statistics",
        "statistics",
        "stat/report/hourly.site",
        "statistics"
    ),
    list!(
        "unifi_get_gateway_stats",
        "Gateway Statistics",
        "statistics",
        "stat/health",
        "statistics"
    ),
    list!(
        "unifi_get_top_clients",
        "Top Clients",
        "statistics",
        "stat/sta",
        "clients"
    ),
    list!(
        "unifi_get_dpi_stats",
        "DPI Statistics",
        "statistics",
        "stat/sitedpi",
        "statistics"
    ),
    list!(
        "unifi_get_site_dpi_traffic",
        "Site DPI Traffic",
        "statistics",
        "stat/sitedpi",
        "traffic"
    ),
    list!(
        "unifi_get_speedtest_results",
        "Speed Test Results",
        "statistics",
        "stat/speedtest-result",
        "results"
    ),
    list!(
        "unifi_get_site_settings",
        "Site Settings",
        "system",
        "get/setting",
        "settings"
    ),
    list!(
        "unifi_get_mgmt_settings",
        "Management Settings",
        "system",
        "get/setting/mgmt",
        "settings"
    ),
    list!(
        "unifi_get_snmp_settings",
        "SNMP Settings",
        "system",
        "get/setting/snmp",
        "settings"
    ),
    list!(
        "unifi_get_system_info",
        "System Information",
        "system",
        "stat/sysinfo",
        "system"
    ),
    list!(
        "unifi_list_backups",
        "Backups",
        "system",
        "cmd/backup",
        "backups"
    ),
    spec!(
        "unifi_raw_network_endpoint",
        "Raw Read-only Endpoint",
        "raw",
        "Call an explicitly allowlisted read-only UniFi Network endpoint.",
        ToolKind::Raw
    ),
];

impl UnifiMcp {
    fn new(unifi: UnifiClient) -> Self {
        Self { unifi }
    }
    fn find_tool(name: &str) -> Option<&'static ToolSpec> {
        TOOLS.iter().find(|tool| tool.name == name)
    }

    async fn dispatch(&self, name: &str, args: Map<String, Value>, nested: bool) -> Value {
        let Some(spec) = Self::find_tool(name) else {
            return error_envelope(format!("Unknown UniFi Network tool '{name}'"));
        };
        if nested
            && matches!(
                spec.kind,
                ToolKind::Index | ToolKind::Execute | ToolKind::Batch
            )
        {
            return error_envelope("Meta-tools cannot recursively execute other meta-tools");
        }
        match self.run(spec, args).await {
            Ok(data) => success_envelope(data),
            Err(err) => error_envelope(format!("Failed to run {name}: {err}")),
        }
    }

    async fn run(&self, spec: &ToolSpec, args: Map<String, Value>) -> Result<Value> {
        match spec.kind {
            ToolKind::Index => Ok(self.tool_index(&args)),
            ToolKind::Execute => {
                let name = required_string(&args, "name")?;
                let inner = object_arg(&args, "arguments")?;
                Ok(Box::pin(self.dispatch(name, inner, true)).await)
            }
            ToolKind::Batch => {
                let calls = args
                    .get("calls")
                    .and_then(Value::as_array)
                    .context("calls must be an array")?;
                if calls.len() > 20 {
                    bail!("batch supports at most 20 calls");
                }
                let mut results = Vec::with_capacity(calls.len());
                for call in calls {
                    let object = call
                        .as_object()
                        .context("each batch call must be an object")?;
                    let name = required_string(object, "name")?;
                    let inner = object_arg(object, "arguments")?;
                    let result = Box::pin(self.dispatch(name, inner, true)).await;
                    results.push(json!({"name": name, "result": result}));
                }
                Ok(json!({"count": results.len(), "results": results}))
            }
            ToolKind::Dashboard => self.dashboard().await,
            ToolKind::List {
                endpoint,
                output_key,
            } => self.list(endpoint, output_key, &args).await,
            ToolKind::Detail {
                endpoint,
                id_arg,
                id_fields,
            } => self.detail(endpoint, id_arg, id_fields, &args).await,
            ToolKind::LookupIp => self.lookup_ip(&args).await,
            ToolKind::Raw => self.raw(&args).await,
        }
    }

    fn tool_index(&self, args: &Map<String, Value>) -> Value {
        let category = optional_string(args, "category").map(str::to_ascii_lowercase);
        let search = optional_string(args, "search").map(str::to_ascii_lowercase);
        let tools = TOOLS
            .iter()
            .filter(|tool| {
                category
                    .as_deref()
                    .is_none_or(|value| tool.category == value)
                    && search.as_deref().is_none_or(|value| {
                        tool.name.to_ascii_lowercase().contains(value)
                            || tool.title.to_ascii_lowercase().contains(value)
                            || tool.description.to_ascii_lowercase().contains(value)
                    })
            })
            .map(|tool| {
                json!({"name":tool.name,"title":tool.title,"category":tool.category,
            "description":tool.description,"read_only":true})
            })
            .collect::<Vec<_>>();
        json!({"count":tools.len(),"tools":tools})
    }

    async fn list(
        &self,
        endpoint: &str,
        output_key: &str,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let limit = bounded_limit(
            args.get("limit")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
        );
        let mut rows = extract_rows_owned(
            self.unifi
                .network_request(Method::GET, endpoint, Some(json!({"_limit":MAX_LIMIT})))
                .await?,
        );
        if output_key == "blocked_clients" {
            rows.retain(|row| row.get("blocked").and_then(Value::as_bool).unwrap_or(false));
        }
        if let Some(query) = optional_string(args, "query")
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            rows.retain(|row| value_contains(row, query));
        }
        let total_count = rows.len();
        let summary = args.get("summary").and_then(Value::as_bool).unwrap_or(true);
        if summary {
            rows = compact_endpoint_rows(endpoint, rows, limit);
        } else {
            rows.truncate(limit);
        }
        Ok(
            json!({"site":self.unifi.site,"total_count":total_count,"returned_count":rows.len(),output_key:rows}),
        )
    }

    async fn detail(
        &self,
        endpoint: &str,
        id_arg: &str,
        id_fields: &[&str],
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        let rows = extract_rows_owned(
            self.unifi
                .network_request(Method::GET, endpoint, Some(json!({"_limit":MAX_LIMIT})))
                .await?,
        );
        rows.into_iter()
            .find(|row| {
                id_fields.iter().any(|field| {
                    row.get(*field)
                        .and_then(Value::as_str)
                        .is_some_and(|v| v.eq_ignore_ascii_case(identifier))
                })
            })
            .with_context(|| format!("No resource matched {id_arg} '{identifier}'"))
    }

    async fn lookup_ip(&self, args: &Map<String, Value>) -> Result<Value> {
        let ip = required_string(args, "ip_address")?;
        let (clients, devices) = tokio::join!(
            self.unifi.network_request(
                Method::GET,
                "stat/alluser",
                Some(json!({"_limit":MAX_LIMIT}))
            ),
            self.unifi.network_request(
                Method::GET,
                "stat/device",
                Some(json!({"_limit":MAX_LIMIT}))
            )
        );
        let mut matches = Vec::new();
        for (kind, payload) in [("client", clients?), ("device", devices?)] {
            for row in extract_rows_owned(payload) {
                if row.get("ip").and_then(Value::as_str) == Some(ip) {
                    matches.push(json!({"kind":kind,"data":row}));
                }
            }
        }
        Ok(json!({"ip_address":ip,"count":matches.len(),"matches":matches}))
    }

    async fn dashboard(&self) -> Result<Value> {
        let health = self
            .unifi
            .network_request(
                Method::GET,
                "stat/health",
                Some(json!({"_limit":MAX_LIMIT})),
            )
            .await?;
        let devices = self
            .unifi
            .network_request(
                Method::GET,
                "stat/device",
                Some(json!({"_limit":MAX_LIMIT})),
            )
            .await?;
        let clients = self
            .unifi
            .network_request(Method::GET, "stat/sta", Some(json!({"_limit":MAX_LIMIT})))
            .await?;
        let wlans = self
            .unifi
            .network_request(
                Method::GET,
                "list/wlanconf",
                Some(json!({"_limit":MAX_LIMIT})),
            )
            .await?;
        let alarms = self
            .unifi
            .network_request(Method::GET, "stat/alarm", Some(json!({"_limit":25})))
            .await?;
        let events = self
            .unifi
            .network_request(Method::GET, "stat/event", Some(json!({"_limit":25})))
            .await?;
        Ok(
            json!({"site":self.unifi.site,"counts":{"devices":extract_rows_owned(devices.clone()).len(),"online_clients":extract_rows_owned(clients).len(),"wlans":extract_rows_owned(wlans.clone()).len(),"alarms":extract_rows_owned(alarms).len(),"events":extract_rows_owned(events).len()},"health":extract_rows_owned(health),"devices":compact_rows(extract_rows_owned(devices),50,&["name","model","type","mac","ip","version","state","adopted"]),"wlans":compact_rows(extract_rows_owned(wlans),50,&["name","enabled","security","wlan_band","_id"])}),
        )
    }

    async fn raw(&self, args: &Map<String, Value>) -> Result<Value> {
        const ALLOWED: &[&str] = &[
            "stat/sta",
            "stat/alluser",
            "stat/rogueap",
            "list/wlanconf",
            "stat/device",
            "stat/health",
            "stat/event",
            "stat/alarm",
            "rest/networkconf",
            "rest/portforward",
            "rest/routing",
            "rest/firewallgroup",
            "rest/firewallrule",
            "rest/portconf",
            "list/usergroup",
            "get/setting",
            "stat/sysinfo",
            "stat/sitedpi",
        ];
        let endpoint = required_string(args, "endpoint")?
            .trim()
            .trim_start_matches('/');
        if !ALLOWED.contains(&endpoint) {
            bail!("endpoint is not in the read-only allowlist");
        }
        let limit = bounded_limit(
            args.get("limit")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
        );
        let payload = self
            .unifi
            .network_request(Method::GET, endpoint, Some(json!({"_limit":limit})))
            .await?;
        Ok(truncate_payload(payload, limit))
    }
}

impl ServerHandler for UnifiMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions("Rust UniFi Network MCP with upstream-compatible discovery, response envelopes, and read-only diagnostics.")
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools: TOOLS.iter().map(tool_model).collect(),
            meta: None,
            next_cursor: None,
            ttl_ms: None,
            cache_scope: None,
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::find_tool(name).map(tool_model)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let value = self
            .dispatch(&request.name, request.arguments.unwrap_or_default(), false)
            .await;
        Ok(CallToolResult::structured(value).into())
    }
}

impl UnifiConfig {
    fn from_env() -> Result<Self> {
        let base_url = resolve_base_url()?;
        let site =
            env_pair("UNIFI_NETWORK_SITE", "UNIFI_SITE").unwrap_or_else(|| DEFAULT_SITE.into());
        let api_key = optional_secret_env_pair("UNIFI_NETWORK_API_KEY", "UNIFI_API_KEY")?;
        let username = env_pair("UNIFI_NETWORK_USERNAME", "UNIFI_USERNAME");
        let password = optional_secret_env_pair("UNIFI_NETWORK_PASSWORD", "UNIFI_PASSWORD")?;
        if api_key.is_none() && (username.is_none() || password.is_none()) {
            bail!("configure an API key or both a local username and password");
        }
        if username.is_some() != password.is_some() {
            bail!("local authentication requires both username and password");
        }
        let insecure_tls =
            if let Some(verify) = env_pair("UNIFI_NETWORK_VERIFY_SSL", "UNIFI_VERIFY_SSL") {
                !parse_bool_value("UNIFI_NETWORK_VERIFY_SSL/UNIFI_VERIFY_SSL", &verify)?
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
            username,
            password,
            insecure_tls,
            redact_sensitive_fields,
        })
    }
}

impl UnifiClient {
    fn new(config: UnifiConfig) -> Result<Self> {
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
            redact_sensitive_fields: config.redact_sensitive_fields,
        })
    }
    async fn ensure_login(&self) -> Result<()> {
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
    async fn network_request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
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
            let mut value = parse_json_response(status, text)?;
            if self.redact_sensitive_fields {
                redact_sensitive(&mut value);
            }
            return Ok(value);
        }
        unreachable!()
    }
    fn network_url(&self, endpoint: &str) -> Result<Url> {
        self.proxy_url_path(&format!(
            "network/api/s/{}/{}",
            self.site,
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

fn tool_model(spec: &ToolSpec) -> Tool {
    let input_schema = match spec.kind {
        ToolKind::Index => schema(
            json!({"category":{"type":"string"},"search":{"type":"string"}}),
            &[],
        ),
        ToolKind::Execute => schema(
            json!({"name":{"type":"string"},"arguments":{"type":"object"}}),
            &["name"],
        ),
        ToolKind::Batch => schema(
            json!({"calls":{"type":"array","maxItems":20,"items":{"type":"object","required":["name"],"properties":{"name":{"type":"string"},"arguments":{"type":"object"}}}}}),
            &["calls"],
        ),
        ToolKind::Detail { id_arg, .. } => schema(json!({id_arg:{"type":"string"}}), &[id_arg]),
        ToolKind::LookupIp => schema(json!({"ip_address":{"type":"string"}}), &["ip_address"]),
        ToolKind::Raw => schema(
            json!({"endpoint":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":500}}),
            &["endpoint"],
        ),
        ToolKind::List { .. } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"query":{"type":"string"},"summary":{"type":"boolean","description":"Return compact records. Defaults to true; set false only when the full selected controller record is required."}}),
            &[],
        ),
        ToolKind::Dashboard => schema(json!({}), &[]),
    };
    Tool::new(
        Cow::Borrowed(spec.name),
        Cow::Borrowed(spec.description),
        Arc::new(input_schema),
    )
    .with_title(spec.title)
    .with_annotations(
        ToolAnnotations::with_title(spec.title)
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}
fn schema(properties: Value, required: &[&str]) -> JsonObject {
    let mut object = Map::new();
    object.insert("type".into(), json!("object"));
    object.insert("additionalProperties".into(), json!(false));
    object.insert("properties".into(), properties);
    if !required.is_empty() {
        object.insert("required".into(), json!(required));
    }
    object
}
fn success_envelope(data: Value) -> Value {
    if data.get("success").is_some() {
        data
    } else {
        json!({"success":true,"data":data})
    }
}
fn error_envelope(message: impl Into<String>) -> Value {
    json!({"success":false,"error":message.into()})
}
fn required_string<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .with_context(|| format!("{key} must be a non-empty string"))
}
fn optional_string<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn object_arg(args: &Map<String, Value>, key: &str) -> Result<Map<String, Value>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(v)) => Ok(v.clone()),
        _ => bail!("{key} must be an object"),
    }
}
fn bounded_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}
fn value_contains(value: &Value, query: &str) -> bool {
    value
        .to_string()
        .to_ascii_lowercase()
        .contains(&query.to_ascii_lowercase())
}
fn extract_rows_owned(value: Value) -> Vec<Value> {
    if let Some(rows) = value.get("data").and_then(Value::as_array) {
        rows.clone()
    } else if let Some(rows) = value.get("items").and_then(Value::as_array) {
        rows.clone()
    } else if let Value::Array(rows) = value {
        rows
    } else {
        vec![value]
    }
}
fn compact_rows(rows: Vec<Value>, limit: usize, fields: &[&str]) -> Vec<Value> {
    rows.into_iter()
        .take(limit)
        .map(|row| {
            let Some(obj) = row.as_object() else {
                return row;
            };
            let mut out = Map::new();
            for field in fields {
                if let Some(value) = obj.get(*field) {
                    out.insert((*field).into(), value.clone());
                }
            }
            Value::Object(out)
        })
        .collect()
}
fn compact_endpoint_rows(endpoint: &str, rows: Vec<Value>, limit: usize) -> Vec<Value> {
    let fields: &[&str] = match endpoint {
        "stat/device" => &[
            "_id", "name", "model", "type", "mac", "ip", "version", "state", "adopted", "uptime",
        ],
        "stat/sta" | "stat/alluser" => &[
            "_id",
            "name",
            "hostname",
            "mac",
            "ip",
            "oui",
            "essid",
            "is_wired",
            "signal",
            "rssi",
            "uptime",
            "last_seen",
            "blocked",
        ],
        "list/wlanconf" => &[
            "_id",
            "name",
            "enabled",
            "security",
            "wlan_band",
            "networkconf_id",
            "is_guest",
        ],
        "stat/rogueap" => &[
            "essid",
            "bssid",
            "channel",
            "signal",
            "rssi",
            "oui",
            "last_seen",
            "first_seen",
            "is_adhoc",
        ],
        "stat/event" | "stat/alarm" => &[
            "_id",
            "key",
            "msg",
            "datetime",
            "time",
            "last_seen",
            "archived",
            "severity",
            "subsystem",
        ],
        "stat/health" => &[
            "subsystem",
            "status",
            "num_adopted",
            "num_disconnected",
            "num_sta",
            "uptime",
            "wan_ip",
            "gw_name",
        ],
        "rest/networkconf" => &[
            "_id",
            "name",
            "purpose",
            "enabled",
            "vlan",
            "ip_subnet",
            "networkgroup",
        ],
        "rest/portforward" => &[
            "_id",
            "name",
            "enabled",
            "dst_port",
            "fwd",
            "proto",
            "wan_interface",
        ],
        "rest/routing" => &["_id", "name", "enabled", "network", "static_ip", "type"],
        "rest/firewallgroup" => &["_id", "name", "group_type", "group_members"],
        "rest/firewallrule" => &[
            "_id",
            "name",
            "enabled",
            "action",
            "ruleset",
            "protocol",
            "src_address",
            "dst_address",
        ],
        "rest/portconf" => &[
            "_id",
            "name",
            "forward",
            "native_networkconf_id",
            "poe_mode",
            "speed",
        ],
        "list/usergroup" => &["_id", "name", "qos_rate_max_down", "qos_rate_max_up"],
        _ => &[],
    };
    if fields.is_empty() {
        rows.into_iter().take(limit).collect()
    } else {
        compact_rows(rows, limit, fields)
    }
}
fn truncate_payload(mut payload: Value, limit: usize) -> Value {
    if let Some(rows) = payload.get_mut("data").and_then(Value::as_array_mut) {
        rows.truncate(limit);
    }
    payload
}
fn parse_json_response(status: StatusCode, body: String) -> Result<Value> {
    if !status.is_success() {
        bail!("UniFi API returned HTTP {status}");
    }
    serde_json::from_str(&body).context("UniFi API returned a non-JSON response")
}
fn resolve_base_url() -> Result<Url> {
    if let Some(value) = env_pair("UNIFI_NETWORK_BASE_URL", "UNIFI_BASE_URL") {
        return Url::parse(&value).with_context(|| format!("invalid UniFi base URL: {value}"));
    }
    if let Some(host) = env_pair("UNIFI_NETWORK_HOST", "UNIFI_HOST") {
        let value = if host.starts_with("http://") || host.starts_with("https://") {
            host
        } else {
            format!(
                "https://{host}:{}",
                env_pair("UNIFI_NETWORK_PORT", "UNIFI_PORT").unwrap_or_else(|| "443".into())
            )
        };
        return Url::parse(&value).context("invalid UniFi host");
    }
    Url::parse(DEFAULT_BASE_URL).context("invalid default URL")
}
fn env_pair(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| env::var(fallback).ok().filter(|v| !v.is_empty()))
}
fn optional_secret_env_pair(primary: &str, fallback: &str) -> Result<Option<String>> {
    if let Some(v) = read_secret_env(primary)? {
        return Ok(Some(v));
    }
    read_secret_env(fallback)
}
fn read_secret_env(name: &str) -> Result<Option<String>> {
    let direct = env::var(name).ok().filter(|v| !v.is_empty());
    let file_name = format!("{name}_FILE");
    let file = env::var(&file_name).ok().filter(|v| !v.is_empty());
    match (direct, file) {
        (Some(_), Some(_)) => bail!("set only one of {name} or {file_name}"),
        (Some(v), None) => Ok(Some(v)),
        (None, Some(path)) => {
            let value =
                fs::read_to_string(path).with_context(|| format!("failed to read {file_name}"))?;
            let value = value.trim_end_matches(['\n', '\r']);
            if value.is_empty() || value.contains(['\n', '\r']) || value.len() > 8192 {
                bail!("{file_name} must point to a short, non-empty, single-line file");
            }
            Ok(Some(value.into()))
        }
        (None, None) => Ok(None),
    }
}
fn parse_bool_env_pair(primary: &str, fallback: &str, default: bool) -> Result<bool> {
    env_pair(primary, fallback).map_or(Ok(default), |v| {
        parse_bool_value(&format!("{primary}/{fallback}"), &v)
    })
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
        Value::Object(obj) => {
            for (key, child) in obj {
                if is_sensitive_field(key) && !child.is_boolean() && !child.is_null() {
                    *child = Value::String(REDACTED.into())
                } else {
                    redact_sensitive(child)
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive(item)
            }
        }
        _ => {}
    }
}
fn is_sensitive_field(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    if key == "token_count"
        || key == "token_counts"
        || key.ends_with("_token_count")
        || key.ends_with("_token_counts")
    {
        return false;
    }
    matches!(
        key.as_str(),
        "auth"
            | "password"
            | "passphrase"
            | "x_passphrase"
            | "x_password"
            | "x_ssh_password"
            | "wep_key"
            | "wpa_key"
            | "psk"
            | "pin"
            | "private_key"
            | "privatekey"
            | "private_preshared_keys"
            | "privatepresharedkeys"
            | "preshared_key"
            | "presharedkey"
            | "api_key"
            | "apikey"
            | "api_token"
            | "auth_key"
            | "authkey"
            | "token"
            | "mgmt_key"
            | "management_key"
            | "x_mgmt_key"
            | "ssh_key"
            | "sshkey"
            | "snmp_community"
            | "encryption_key"
            | "vpn_config"
            | "wireguard_config"
            | "ovpn"
            | "x_iapp_key"
            | "x_authkey"
            | "x_auth_key"
            | "x_inform_authkey"
            | "x_vwirekey"
            | "x_ca_key"
            | "x_server_key"
            | "x_shared_client_key"
            | "syslog_key"
            | "community"
            | "tls_auth"
            | "tls_crypt"
            | "pin_code"
            | "rtsp_alias"
            | "rtsp_url"
            | "rtsps_url"
            | "rtsps_streams"
            | "openvpn_configuration"
            | "wireguard_client_configuration_file"
            | "wireguard_server_configuration_file"
    ) || key
        .split(|c: char| !c.is_ascii_alphanumeric() || c == '_')
        .any(|part| {
            matches!(
                part,
                "password"
                    | "passwd"
                    | "passphrase"
                    | "psk"
                    | "secret"
                    | "token"
                    | "authorization"
                    | "cookie"
            )
        })
        || key.ends_with("_password")
        || key.ends_with("_secret")
        || key.ends_with("_private_key")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let server = UnifiMcp::new(UnifiClient::new(UnifiConfig::from_env()?)?);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_limits() {
        assert_eq!(bounded_limit(None), 100);
        assert_eq!(bounded_limit(Some(0)), 1);
        assert_eq!(bounded_limit(Some(501)), 500);
    }
    #[test]
    fn redacts_secrets_without_redacting_flags() {
        let mut value = json!({"x_passphrase":"secret","passphrase_autogenerated":true,"private_preshared_keys_enabled":true,"nested":{"api_key":"secret"}});
        redact_sensitive(&mut value);
        assert_eq!(value["x_passphrase"], REDACTED);
        assert_eq!(value["nested"]["api_key"], REDACTED);
        assert_eq!(value["passphrase_autogenerated"], true);
        assert_eq!(value["private_preshared_keys_enabled"], true);
    }
    #[test]
    fn redacts_controller_key_material_and_tokens() {
        let mut value = json!({
            "x_authkey": "controller-auth-key",
            "x_iapp_key": "iapp-key",
            "x_vwirekey": "vwire-key",
            "guest_token": "guest-token",
            "private_preshared_keys": ["psk"],
            "sae_psk": ["sae-psk"],
            "token_count": 7,
        });
        redact_sensitive(&mut value);
        for key in [
            "x_authkey",
            "x_iapp_key",
            "x_vwirekey",
            "guest_token",
            "private_preshared_keys",
            "sae_psk",
        ] {
            assert_eq!(value[key], REDACTED);
        }
        assert_eq!(value["token_count"], 7);
    }
    #[test]
    fn catalog_names_are_unique() {
        let mut names = TOOLS.iter().map(|t| t.name).collect::<Vec<_>>();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }
    #[test]
    fn catalog_is_read_only() {
        for tool in TOOLS {
            let model = tool_model(tool);
            assert_eq!(model.annotations.unwrap().read_only_hint, Some(true));
        }
    }
}
