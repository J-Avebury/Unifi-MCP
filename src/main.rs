use anyhow::{Context, Result, bail};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use reqwest::{Client, Method, StatusCode, Url};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, JsonObject, ListToolsResult,
        PaginatedRequestParams, ResultType, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::{
        stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use serde_json::{Map, Value, json};
use std::{
    borrow::Cow,
    convert::Infallible,
    env, fs,
    net::SocketAddr,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_service::Service;

mod client;
mod legacy;
mod site_manager;
mod tools;

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
macro_rules! filtered_list {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $key:expr, $field:expr, $value:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the configured UniFi Network site."),
            ToolKind::FilteredList {
                endpoint: $endpoint,
                output_key: $key,
                filter_field: $field,
                filter_value: $value,
            }
        )
    };
}
macro_rules! filtered_detail {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $arg:expr, $fields:expr, $filter_field:expr, $filter_value:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Return ",
                $title,
                " from the configured UniFi Network site."
            ),
            ToolKind::FilteredDetail {
                endpoint: $endpoint,
                id_arg: $arg,
                id_fields: $fields,
                filter_field: $filter_field,
                filter_value: $filter_value,
            }
        )
    };
}
macro_rules! action {
    ($name:expr, $title:expr, $category:expr, $description:expr, $endpoint:expr, $command:expr, $id_arg:expr, $destructive:expr, $idempotent:expr) => {
        spec!(
            $name,
            $title,
            $category,
            $description,
            ToolKind::Action {
                endpoint: $endpoint,
                command: $command,
                id_arg: $id_arg,
                destructive: $destructive,
                idempotent: $idempotent,
            }
        )
    };
}
macro_rules! v2_list {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $key:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the UniFi Network Integration API."),
            ToolKind::V2List {
                endpoint: $endpoint,
                output_key: $key
            }
        )
    };
}
macro_rules! v2_detail {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $arg:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Return ",
                $title,
                " from the UniFi Network Integration API."
            ),
            ToolKind::V2Detail {
                endpoint: $endpoint,
                id_arg: $arg
            }
        )
    };
}
macro_rules! v2_nested_detail {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $arg:expr, $suffix:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Return ",
                $title,
                " from the UniFi Network Integration API."
            ),
            ToolKind::V2NestedDetail {
                endpoint: $endpoint,
                id_arg: $arg,
                suffix: $suffix,
            }
        )
    };
}
macro_rules! integration_list {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr, $key:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the UniFi Network Integration API."),
            ToolKind::IntegrationList {
                endpoint: $endpoint,
                output_key: $key,
            }
        )
    };
}
macro_rules! integration_object {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the UniFi Network Integration API."),
            ToolKind::IntegrationObject {
                endpoint: $endpoint
            }
        )
    };
}
macro_rules! integration_query {
    ($name:expr, $title:expr, $category:expr, $endpoint:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!("Read ", $title, " from the UniFi Network Integration API."),
            ToolKind::IntegrationQuery {
                endpoint: $endpoint
            }
        )
    };
}
macro_rules! special_read {
    ($name:expr, $title:expr, $category:expr, $operation:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Read ",
                $title,
                " using the controller's documented Network contract."
            ),
            ToolKind::SpecialRead {
                operation: $operation
            }
        )
    };
}
macro_rules! integration_write {
    ($name:expr, $title:expr, $category:expr, $method:expr, $endpoint:expr, $id_arg:expr, $body_required:expr) => {
        spec!(
            $name,
            $title,
            $category,
            concat!(
                "Preview and, after confirmation, perform ",
                $title,
                " through the UniFi Network Integration API."
            ),
            ToolKind::IntegrationWrite {
                method: $method,
                endpoint: $endpoint,
                id_arg: $id_arg,
                body_required: $body_required,
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
    integration_site_id: Arc<Mutex<Option<String>>>,
    site_manager: Option<site_manager::SiteManagerClient>,
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
    site_manager_api_key: Option<String>,
    site_manager_site_id: Option<String>,
    site_manager_console_id: Option<String>,
}

#[derive(Clone, Copy)]
enum ToolKind {
    Index,
    Execute,
    Batch,
    Dashboard,
    SpecialRead {
        operation: SpecialReadOperation,
    },
    List {
        endpoint: &'static str,
        output_key: &'static str,
    },
    Detail {
        endpoint: &'static str,
        id_arg: &'static str,
        id_fields: &'static [&'static str],
    },
    FilteredList {
        endpoint: &'static str,
        output_key: &'static str,
        filter_field: &'static str,
        filter_value: &'static str,
    },
    FilteredDetail {
        endpoint: &'static str,
        id_arg: &'static str,
        id_fields: &'static [&'static str],
        filter_field: &'static str,
        filter_value: &'static str,
    },
    LookupIp,
    Raw,
    ClientRecord,
    ClientBandwidth,
    V2List {
        endpoint: &'static str,
        output_key: &'static str,
    },
    V2Detail {
        endpoint: &'static str,
        id_arg: &'static str,
    },
    V2NestedDetail {
        endpoint: &'static str,
        id_arg: &'static str,
        suffix: &'static str,
    },
    IntegrationList {
        endpoint: &'static str,
        output_key: &'static str,
    },
    IntegrationObject {
        endpoint: &'static str,
    },
    IntegrationQuery {
        endpoint: &'static str,
    },
    IntegrationWrite {
        method: IntegrationMethod,
        endpoint: &'static str,
        id_arg: Option<&'static str>,
        body_required: bool,
    },
    LegacyWlanUpdate,
    Action {
        endpoint: &'static str,
        command: &'static str,
        id_arg: &'static str,
        destructive: bool,
        idempotent: bool,
    },
    Compatibility {
        endpoint: &'static str,
        method: CompatibilityMethod,
        id_arg: Option<&'static str>,
        read_only: bool,
        destructive: bool,
        idempotent: bool,
    },
}

#[derive(Clone, Copy)]
enum IntegrationMethod {
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Copy)]
enum SpecialReadOperation {
    BatchStatus,
    Dashboard,
    Events,
    EventTypes,
    Alarms,
    RecentEvents,
    SubscribeEvents,
    IpsEvents,
    SpeedtestResults,
    TrafficFlows,
    TrafficFlowStatistics,
    Backups,
}

#[derive(Clone, Copy)]
struct ToolSpec {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) category: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: ToolKind,
}

#[derive(Clone, Copy)]
pub(crate) enum CompatibilityMethod {
    Get,
    Post,
    Put,
    Delete,
}

const DEVICE_IDS: &[&str] = &["_id", "id", "mac"];
const CLIENT_IDS: &[&str] = &["_id", "id", "mac"];
const CONFIG_IDS: &[&str] = &["_id", "id"];

const BASE_TOOLS: &[ToolSpec] = &[
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
    special_read!(
        "unifi_list_events",
        "List Events",
        "events",
        SpecialReadOperation::Events
    ),
    special_read!(
        "unifi_recent_events",
        "Recent Events",
        "events",
        SpecialReadOperation::RecentEvents
    ),
    special_read!(
        "unifi_list_alarms",
        "List Alarms",
        "events",
        SpecialReadOperation::Alarms
    ),
    special_read!(
        "unifi_get_alerts",
        "Alerts",
        "events",
        SpecialReadOperation::Alarms
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
        "unifi_list_dynamic_dns",
        "Dynamic DNS Entries",
        "dns",
        "rest/dynamicdns",
        "dynamic_dns"
    ),
    detail!(
        "unifi_get_dynamic_dns_entry_details",
        "Dynamic DNS Entry Details",
        "dns",
        "rest/dynamicdns",
        "entry_id",
        CONFIG_IDS
    ),
    list!(
        "unifi_list_vouchers",
        "Hotspot Vouchers",
        "hotspot",
        "stat/voucher",
        "vouchers"
    ),
    detail!(
        "unifi_get_voucher_details",
        "Voucher Details",
        "hotspot",
        "stat/voucher",
        "voucher_id",
        CONFIG_IDS
    ),
    filtered_list!(
        "unifi_list_vpn_clients",
        "VPN Clients",
        "vpn",
        "rest/networkconf",
        "vpn_clients",
        "purpose",
        "vpn-client"
    ),
    filtered_detail!(
        "unifi_get_vpn_client_details",
        "VPN Client Details",
        "vpn",
        "rest/networkconf",
        "client_id",
        CONFIG_IDS,
        "purpose",
        "vpn-client"
    ),
    filtered_list!(
        "unifi_list_vpn_servers",
        "VPN Servers",
        "vpn",
        "rest/networkconf",
        "vpn_servers",
        "purpose",
        "vpn-server"
    ),
    filtered_detail!(
        "unifi_get_vpn_server_details",
        "VPN Server Details",
        "vpn",
        "rest/networkconf",
        "server_id",
        CONFIG_IDS,
        "purpose",
        "vpn-server"
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
    special_read!(
        "unifi_get_speedtest_results",
        "Speed Test Results",
        "statistics",
        SpecialReadOperation::SpeedtestResults
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
        "unifi_get_autobackup_settings",
        "Auto-backup Settings",
        "system",
        "get/setting",
        "settings"
    ),
    list!(
        "unifi_get_gateway_settings",
        "Gateway Settings",
        "system",
        "get/setting",
        "settings"
    ),
    list!(
        "unifi_get_client_stats",
        "Client Statistics",
        "statistics",
        "stat/sta",
        "statistics"
    ),
    spec!(
        "unifi_get_client_record",
        "Client Record",
        "statistics",
        "Return the stored lifetime record for one client, addressed by MAC address.",
        ToolKind::ClientRecord
    ),
    spec!(
        "unifi_get_client_bandwidth",
        "Client Bandwidth History",
        "statistics",
        "Return historical client bandwidth samples for a 5-minute, hourly, or daily interval.",
        ToolKind::ClientBandwidth
    ),
    list!(
        "unifi_get_client_sessions",
        "Client Sessions",
        "statistics",
        "stat/session",
        "sessions"
    ),
    list!(
        "unifi_get_client_dpi_traffic",
        "Client DPI Traffic",
        "statistics",
        "stat/sitedpi",
        "traffic"
    ),
    list!(
        "unifi_get_client_wifi_details",
        "Client Wi-Fi Details",
        "wireless",
        "stat/sta",
        "clients"
    ),
    list!(
        "unifi_get_device_radio",
        "Device Radio",
        "wireless",
        "stat/device",
        "radios"
    ),
    special_read!(
        "unifi_get_event_types",
        "Event Types",
        "events",
        SpecialReadOperation::EventTypes
    ),
    special_read!(
        "unifi_get_ips_events",
        "IPS Events",
        "events",
        SpecialReadOperation::IpsEvents
    ),
    list!(
        "unifi_get_lldp_neighbors",
        "LLDP Neighbours",
        "switch",
        "stat/device",
        "neighbors"
    ),
    list!(
        "unifi_get_pdu_outlets",
        "PDU Outlets",
        "switch",
        "stat/device",
        "outlets"
    ),
    list!(
        "unifi_get_port_stats",
        "Port Statistics",
        "switch",
        "stat/device",
        "ports"
    ),
    list!(
        "unifi_get_rf_scan_results",
        "RF Scan Results",
        "wireless",
        "stat/device",
        "rf_scan"
    ),
    list!(
        "unifi_get_speedtest_status",
        "Speed Test Status",
        "statistics",
        "stat/speedtest",
        "status"
    ),
    list!(
        "unifi_get_support_bundle",
        "Support Bundle",
        "system",
        "stat/sysinfo",
        "support_bundle"
    ),
    list!(
        "unifi_get_switch_capabilities",
        "Switch Capabilities",
        "switch",
        "stat/device",
        "capabilities"
    ),
    list!(
        "unifi_get_switch_ports",
        "Switch Ports",
        "switch",
        "stat/device",
        "ports"
    ),
    special_read!(
        "unifi_get_traffic_flow_statistics",
        "Traffic Flow Statistics",
        "statistics",
        SpecialReadOperation::TrafficFlowStatistics
    ),
    special_read!(
        "unifi_get_traffic_flows",
        "Traffic Flows",
        "statistics",
        SpecialReadOperation::TrafficFlows
    ),
    list!(
        "unifi_list_available_channels",
        "Available Channels",
        "wireless",
        "get/setting",
        "channels"
    ),
    v2_list!(
        "unifi_list_oon_policies",
        "OON Policies",
        "security",
        "object-oriented-network-configs",
        "policies"
    ),
    v2_detail!(
        "unifi_get_oon_policy_details",
        "OON Policy Details",
        "security",
        "object-oriented-network-config",
        "policy_id"
    ),
    list!(
        "unifi_get_system_info",
        "System Information",
        "system",
        "stat/sysinfo",
        "system"
    ),
    special_read!(
        "unifi_list_backups",
        "Backups",
        "system",
        SpecialReadOperation::Backups
    ),
    spec!(
        "unifi_raw_network_endpoint",
        "Raw Read-only Endpoint",
        "raw",
        "Call an explicitly allowlisted read-only UniFi Network endpoint.",
        ToolKind::Raw
    ),
    v2_list!(
        "unifi_list_acl_rules",
        "ACL Rules",
        "acl",
        "acl-rules",
        "acl_rules"
    ),
    v2_detail!(
        "unifi_get_acl_rule_details",
        "ACL Rule Details",
        "acl",
        "acl-rules",
        "acl_rule_id"
    ),
    v2_list!(
        "unifi_list_ap_groups",
        "AP Groups",
        "wireless",
        "apgroups",
        "ap_groups"
    ),
    v2_detail!(
        "unifi_get_ap_group_details",
        "AP Group Details",
        "wireless",
        "apgroups",
        "ap_group_id"
    ),
    v2_list!(
        "unifi_list_client_groups",
        "Client Groups",
        "clients",
        "network-members-groups",
        "client_groups"
    ),
    v2_detail!(
        "unifi_get_client_group_details",
        "Client Group Details",
        "clients",
        "network-members-group",
        "client_group_id"
    ),
    v2_list!(
        "unifi_list_content_filters",
        "Content Filters",
        "security",
        "content-filtering",
        "content_filters"
    ),
    v2_detail!(
        "unifi_get_content_filter_details",
        "Content Filter Details",
        "security",
        "content-filtering",
        "content_filter_id"
    ),
    v2_list!(
        "unifi_list_dns_records",
        "DNS Records",
        "dns",
        "static-dns",
        "dns_records"
    ),
    v2_detail!(
        "unifi_get_dns_record_details",
        "DNS Record Details",
        "dns",
        "static-dns",
        "dns_record_id"
    ),
    v2_list!(
        "unifi_list_firewall_policies",
        "Firewall Policies",
        "firewall",
        "firewall-policies",
        "firewall_policies"
    ),
    v2_detail!(
        "unifi_get_firewall_policy_details",
        "Firewall Policy Details",
        "firewall",
        "firewall-policies",
        "firewall_policy_id"
    ),
    v2_list!(
        "unifi_list_firewall_zones",
        "Firewall Zones",
        "firewall",
        "firewall/zones",
        "firewall_zones"
    ),
    v2_list!(
        "unifi_list_qos_rules",
        "QoS Rules",
        "qos",
        "qos-rules",
        "qos_rules"
    ),
    v2_detail!(
        "unifi_get_qos_rule_details",
        "QoS Rule Details",
        "qos",
        "qos-rules",
        "qos_rule_id"
    ),
    v2_list!(
        "unifi_list_traffic_routes",
        "Traffic Routes",
        "routing",
        "trafficroutes",
        "traffic_routes"
    ),
    v2_detail!(
        "unifi_get_traffic_route_details",
        "Traffic Route Details",
        "routing",
        "trafficroutes",
        "traffic_route_id"
    ),
    v2_list!(
        "unifi_list_switch_stacks",
        "Switch Stacks",
        "switch",
        "switching/switch-stacks",
        "switch_stacks"
    ),
    v2_detail!(
        "unifi_get_switch_stack_details",
        "Switch Stack Details",
        "switch",
        "switching/switch-stacks",
        "switch_stack_id"
    ),
    v2_list!(
        "unifi_list_mc_lag_domains",
        "MC-LAG Domains",
        "switch",
        "switching/mc-lag-domains",
        "mc_lag_domains"
    ),
    v2_detail!(
        "unifi_get_mc_lag_domain_details",
        "MC-LAG Domain Details",
        "switch",
        "switching/mc-lag-domains",
        "mc_lag_domain_id"
    ),
    v2_list!(
        "unifi_list_lags",
        "Link Aggregation Groups",
        "switch",
        "switching/lags",
        "lags"
    ),
    v2_detail!(
        "unifi_get_lag_details",
        "Link Aggregation Group Details",
        "switch",
        "switching/lags",
        "lag_id"
    ),
    v2_list!(
        "unifi_list_dns_policies",
        "DNS Policies",
        "dns",
        "dns/policies",
        "dns_policies"
    ),
    v2_detail!(
        "unifi_get_dns_policy_details",
        "DNS Policy Details",
        "dns",
        "dns/policies",
        "dns_policy_id"
    ),
    v2_list!(
        "unifi_list_adopted_devices",
        "Official Adopted Devices",
        "devices",
        "devices",
        "devices"
    ),
    v2_detail!(
        "unifi_get_adopted_device_details",
        "Official Adopted Device Details",
        "devices",
        "devices",
        "device_id"
    ),
    v2_nested_detail!(
        "unifi_get_adopted_device_statistics",
        "Official Adopted Device Statistics",
        "devices",
        "devices",
        "device_id",
        "statistics/latest"
    ),
    v2_list!(
        "unifi_list_api_clients",
        "Official Connected Clients",
        "clients",
        "clients",
        "clients"
    ),
    v2_detail!(
        "unifi_get_api_client_details",
        "Official Connected Client Details",
        "clients",
        "clients",
        "client_id"
    ),
    v2_list!(
        "unifi_list_api_networks",
        "Official Networks",
        "networks",
        "networks",
        "networks"
    ),
    v2_detail!(
        "unifi_get_api_network_details",
        "Official Network Details",
        "networks",
        "networks",
        "network_id"
    ),
    v2_list!(
        "unifi_list_wifi_broadcasts",
        "Official Wi-Fi Broadcasts",
        "wireless",
        "wifi/broadcasts",
        "wifi_broadcasts"
    ),
    v2_detail!(
        "unifi_get_wifi_broadcast_details",
        "Official Wi-Fi Broadcast Details",
        "wireless",
        "wifi/broadcasts",
        "wifi_broadcast_id"
    ),
    v2_nested_detail!(
        "unifi_get_api_network_references",
        "Official Network References",
        "networks",
        "networks",
        "network_id",
        "references"
    ),
    integration_list!(
        "unifi_list_dpi_applications",
        "DPI Applications",
        "security",
        "v1/dpi/applications",
        "dpi_applications"
    ),
    integration_list!(
        "unifi_list_dpi_categories",
        "DPI Categories",
        "security",
        "v1/dpi/categories",
        "dpi_categories"
    ),
    integration_list!(
        "unifi_list_api_sites",
        "Official Local Sites",
        "system",
        "v1/sites",
        "sites"
    ),
    integration_list!(
        "unifi_list_pending_api_devices",
        "Official Pending Devices",
        "devices",
        "v1/pending-devices",
        "pending_devices"
    ),
    integration_object!(
        "unifi_get_api_application_info",
        "Official Network Application Info",
        "system",
        "v1/info"
    ),
    integration_query!(
        "unifi_get_api_firewall_policy_ordering",
        "Official Firewall Policy Ordering",
        "firewall",
        "firewall/policies/ordering"
    ),
    integration_query!(
        "unifi_get_firewall_policy_ordering",
        "Firewall Policy Ordering",
        "firewall",
        "firewall/policies/ordering"
    ),
    v2_list!(
        "unifi_list_api_vouchers",
        "Official Hotspot Vouchers",
        "hotspot",
        "hotspot/vouchers",
        "vouchers"
    ),
    v2_detail!(
        "unifi_get_api_voucher_details",
        "Official Hotspot Voucher Details",
        "hotspot",
        "hotspot/vouchers",
        "voucher_id"
    ),
    v2_list!(
        "unifi_list_traffic_matching_lists",
        "Official Traffic Matching Lists",
        "firewall",
        "traffic-matching-lists",
        "traffic_matching_lists"
    ),
    v2_detail!(
        "unifi_get_traffic_matching_list_details",
        "Official Traffic Matching List Details",
        "firewall",
        "traffic-matching-lists",
        "traffic_matching_list_id"
    ),
    v2_list!(
        "unifi_list_wan_interfaces",
        "Official WAN Interfaces",
        "routing",
        "wans",
        "wan_interfaces"
    ),
    v2_list!(
        "unifi_list_site_to_site_vpn_tunnels",
        "Official Site-to-Site VPN Tunnels",
        "vpn",
        "vpn/site-to-site-tunnels",
        "site_to_site_vpn_tunnels"
    ),
    v2_list!(
        "unifi_list_api_vpn_servers",
        "Official VPN Servers",
        "vpn",
        "vpn/servers",
        "vpn_servers"
    ),
    v2_list!(
        "unifi_list_radius_profiles",
        "Official RADIUS Profiles",
        "security",
        "radius/profiles",
        "radius_profiles"
    ),
    v2_list!(
        "unifi_list_device_tags",
        "Official Device Tags",
        "devices",
        "device-tags",
        "device_tags"
    ),
    integration_list!(
        "unifi_list_countries",
        "Official Countries",
        "system",
        "v1/countries",
        "countries"
    ),
    integration_write!(
        "unifi_create_network",
        "Create Network",
        "networks",
        IntegrationMethod::Post,
        "networks",
        None,
        true
    ),
    integration_write!(
        "unifi_update_network",
        "Update Network",
        "networks",
        IntegrationMethod::Put,
        "networks/{id}",
        Some("network_id"),
        true
    ),
    integration_write!(
        "unifi_delete_network",
        "Delete Network",
        "networks",
        IntegrationMethod::Delete,
        "networks/{id}",
        Some("network_id"),
        false
    ),
    integration_write!(
        "unifi_create_dns_policy",
        "Create DNS Policy",
        "dns",
        IntegrationMethod::Post,
        "dns/policies",
        None,
        true
    ),
    integration_write!(
        "unifi_update_dns_policy",
        "Update DNS Policy",
        "dns",
        IntegrationMethod::Put,
        "dns/policies/{id}",
        Some("dns_policy_id"),
        true
    ),
    integration_write!(
        "unifi_delete_dns_policy",
        "Delete DNS Policy",
        "dns",
        IntegrationMethod::Delete,
        "dns/policies/{id}",
        Some("dns_policy_id"),
        false
    ),
    integration_write!(
        "unifi_create_wifi_broadcast",
        "Create Wi-Fi Broadcast",
        "wireless",
        IntegrationMethod::Post,
        "wifi/broadcasts",
        None,
        true
    ),
    integration_write!(
        "unifi_update_wifi_broadcast",
        "Update Wi-Fi Broadcast",
        "wireless",
        IntegrationMethod::Put,
        "wifi/broadcasts/{id}",
        Some("wifi_broadcast_id"),
        true
    ),
    integration_write!(
        "unifi_delete_wifi_broadcast",
        "Delete Wi-Fi Broadcast",
        "wireless",
        IntegrationMethod::Delete,
        "wifi/broadcasts/{id}",
        Some("wifi_broadcast_id"),
        false
    ),
    integration_write!(
        "unifi_generate_vouchers",
        "Generate Hotspot Vouchers",
        "hotspot",
        IntegrationMethod::Post,
        "hotspot/vouchers",
        None,
        true
    ),
    integration_write!(
        "unifi_delete_voucher",
        "Delete Hotspot Voucher",
        "hotspot",
        IntegrationMethod::Delete,
        "hotspot/vouchers/{id}",
        Some("voucher_id"),
        false
    ),
    integration_write!(
        "unifi_create_firewall_zone",
        "Create Firewall Zone",
        "firewall",
        IntegrationMethod::Post,
        "firewall/zones",
        None,
        true
    ),
    integration_write!(
        "unifi_update_firewall_zone",
        "Update Firewall Zone",
        "firewall",
        IntegrationMethod::Put,
        "firewall/zones/{id}",
        Some("firewall_zone_id"),
        true
    ),
    integration_write!(
        "unifi_delete_firewall_zone",
        "Delete Firewall Zone",
        "firewall",
        IntegrationMethod::Delete,
        "firewall/zones/{id}",
        Some("firewall_zone_id"),
        false
    ),
    integration_write!(
        "unifi_create_firewall_policy",
        "Create Firewall Policy",
        "firewall",
        IntegrationMethod::Post,
        "firewall/policies",
        None,
        true
    ),
    integration_write!(
        "unifi_update_firewall_policy",
        "Update Firewall Policy",
        "firewall",
        IntegrationMethod::Put,
        "firewall/policies/{id}",
        Some("firewall_policy_id"),
        true
    ),
    integration_write!(
        "unifi_delete_firewall_policy",
        "Delete Firewall Policy",
        "firewall",
        IntegrationMethod::Delete,
        "firewall/policies/{id}",
        Some("firewall_policy_id"),
        false
    ),
    integration_write!(
        "unifi_patch_firewall_policy",
        "Patch Firewall Policy",
        "firewall",
        IntegrationMethod::Patch,
        "firewall/policies/{id}",
        Some("firewall_policy_id"),
        true
    ),
    integration_write!(
        "unifi_reorder_api_firewall_policies",
        "Reorder Firewall Policies",
        "firewall",
        IntegrationMethod::Put,
        "firewall/policies/ordering",
        None,
        true
    ),
    integration_write!(
        "unifi_reorder_firewall_policies",
        "Reorder Firewall Policies",
        "firewall",
        IntegrationMethod::Put,
        "firewall/policies/ordering",
        None,
        true
    ),
    integration_write!(
        "unifi_create_acl_rule",
        "Create ACL Rule",
        "acl",
        IntegrationMethod::Post,
        "acl-rules",
        None,
        true
    ),
    integration_write!(
        "unifi_update_acl_rule",
        "Update ACL Rule",
        "acl",
        IntegrationMethod::Put,
        "acl-rules/{id}",
        Some("acl_rule_id"),
        true
    ),
    integration_write!(
        "unifi_delete_acl_rule",
        "Delete ACL Rule",
        "acl",
        IntegrationMethod::Delete,
        "acl-rules/{id}",
        Some("acl_rule_id"),
        false
    ),
    integration_write!(
        "unifi_reorder_api_acl_rules",
        "Reorder ACL Rules",
        "acl",
        IntegrationMethod::Put,
        "acl-rules/ordering",
        None,
        true
    ),
    integration_write!(
        "unifi_create_traffic_matching_list",
        "Create Traffic Matching List",
        "firewall",
        IntegrationMethod::Post,
        "traffic-matching-lists",
        None,
        true
    ),
    integration_write!(
        "unifi_update_traffic_matching_list",
        "Update Traffic Matching List",
        "firewall",
        IntegrationMethod::Put,
        "traffic-matching-lists/{id}",
        Some("traffic_matching_list_id"),
        true
    ),
    integration_write!(
        "unifi_delete_traffic_matching_list",
        "Delete Traffic Matching List",
        "firewall",
        IntegrationMethod::Delete,
        "traffic-matching-lists/{id}",
        Some("traffic_matching_list_id"),
        false
    ),
    integration_write!(
        "unifi_delete_api_vouchers",
        "Delete Hotspot Vouchers",
        "hotspot",
        IntegrationMethod::Delete,
        "hotspot/vouchers",
        None,
        false
    ),
    integration_write!(
        "unifi_adopt_api_device",
        "Adopt Device",
        "devices",
        IntegrationMethod::Post,
        "devices",
        None,
        true
    ),
    integration_write!(
        "unifi_adopt_device",
        "Adopt Device",
        "devices",
        IntegrationMethod::Post,
        "devices",
        None,
        true
    ),
    integration_write!(
        "unifi_remove_api_device",
        "Remove Adopted Device",
        "devices",
        IntegrationMethod::Delete,
        "devices/{id}",
        Some("device_id"),
        false
    ),
    integration_write!(
        "unifi_force_provision_device",
        "Force Provision Device",
        "devices",
        IntegrationMethod::Post,
        "devices/{id}/actions",
        Some("device_id"),
        true
    ),
    integration_write!(
        "unifi_execute_api_device_action",
        "Execute Adopted Device Action",
        "devices",
        IntegrationMethod::Post,
        "devices/{id}/actions",
        Some("device_id"),
        true
    ),
    integration_write!(
        "unifi_power_cycle_port",
        "Power Cycle Device Port",
        "switch",
        IntegrationMethod::Post,
        "devices/{id}/interfaces/ports/{port}/actions",
        Some("device_id"),
        true
    ),
    integration_write!(
        "unifi_execute_api_port_action",
        "Execute Device Port Action",
        "switch",
        IntegrationMethod::Post,
        "devices/{id}/interfaces/ports/{port}/actions",
        Some("device_id"),
        true
    ),
    integration_write!(
        "unifi_execute_api_client_action",
        "Execute Client Action",
        "clients",
        IntegrationMethod::Post,
        "clients/{id}/actions",
        Some("client_id"),
        true
    ),
    integration_write!(
        "unifi_authorize_guest",
        "Authorize Guest Client",
        "clients",
        IntegrationMethod::Post,
        "clients/{id}/actions",
        Some("client_id"),
        true
    ),
    integration_write!(
        "unifi_unauthorize_guest",
        "Unauthorize Guest Client",
        "clients",
        IntegrationMethod::Post,
        "clients/{id}/actions",
        Some("client_id"),
        true
    ),
    integration_write!(
        "unifi_revoke_voucher",
        "Revoke Hotspot Voucher",
        "hotspot",
        IntegrationMethod::Delete,
        "hotspot/vouchers/{id}",
        Some("voucher_id"),
        false
    ),
    integration_write!(
        "unifi_create_wlan",
        "Create WLAN",
        "wireless",
        IntegrationMethod::Post,
        "wifi/broadcasts",
        None,
        true
    ),
    spec!(
        "unifi_update_wlan",
        "Update WLAN",
        "wireless",
        "Update WLAN fields through the legacy controller API with fetch-merge-write and read-back verification.",
        ToolKind::LegacyWlanUpdate
    ),
    integration_write!(
        "unifi_delete_wlan",
        "Delete WLAN",
        "wireless",
        IntegrationMethod::Delete,
        "wifi/broadcasts/{id}",
        Some("wifi_broadcast_id"),
        false
    ),
    action!(
        "unifi_block_client",
        "Block Client",
        "clients",
        "Block a client. Returns a preview unless confirm is true.",
        "cmd/stamgr",
        "block-sta",
        "client_mac",
        true,
        true
    ),
    action!(
        "unifi_unblock_client",
        "Unblock Client",
        "clients",
        "Unblock a client. Returns a preview unless confirm is true.",
        "cmd/stamgr",
        "unblock-sta",
        "client_mac",
        false,
        true
    ),
    action!(
        "unifi_force_reconnect_client",
        "Reconnect Client",
        "clients",
        "Disconnect a client so it reconnects. Returns a preview unless confirm is true.",
        "cmd/stamgr",
        "kick-sta",
        "client_mac",
        true,
        false
    ),
    action!(
        "unifi_reboot_device",
        "Reboot Device",
        "devices",
        "Reboot a managed device. Returns a preview unless confirm is true.",
        "cmd/devmgr",
        "restart",
        "device_mac",
        true,
        false
    ),
    action!(
        "unifi_upgrade_device",
        "Upgrade Device",
        "devices",
        "Start a managed device firmware upgrade. Returns a preview unless confirm is true.",
        "cmd/devmgr",
        "upgrade",
        "device_mac",
        true,
        false
    ),
];

impl UnifiMcp {
    fn new(unifi: UnifiClient) -> Self {
        Self { unifi }
    }
    fn find_tool(name: &str) -> Option<&'static ToolSpec> {
        tools::find(name)
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
                    if Self::find_tool(name).is_some_and(|tool| {
                        matches!(
                            tool.kind,
                            ToolKind::Action { .. }
                                | ToolKind::IntegrationWrite { .. }
                                | ToolKind::LegacyWlanUpdate
                                | ToolKind::Compatibility {
                                    read_only: false,
                                    ..
                                }
                        )
                    }) {
                        results.push(json!({"name": name, "result": error_envelope("unifi_batch accepts read-only tools only")}));
                        continue;
                    }
                    let result = Box::pin(self.dispatch(name, inner, true)).await;
                    results.push(json!({"name": name, "result": result}));
                }
                Ok(json!({"count": results.len(), "results": results}))
            }
            ToolKind::Dashboard => {
                self.special_read(SpecialReadOperation::Dashboard, &args)
                    .await
            }
            ToolKind::SpecialRead { operation } => self.special_read(operation, &args).await,
            ToolKind::List {
                endpoint,
                output_key,
            } => self.list(endpoint, output_key, &args).await,
            ToolKind::Detail {
                endpoint,
                id_arg,
                id_fields,
            } => self.detail(endpoint, id_arg, id_fields, &args).await,
            ToolKind::FilteredList {
                endpoint,
                output_key,
                filter_field,
                filter_value,
            } => {
                self.filtered_list(endpoint, output_key, filter_field, filter_value, &args)
                    .await
            }
            ToolKind::FilteredDetail {
                endpoint,
                id_arg,
                id_fields,
                filter_field,
                filter_value,
            } => {
                self.filtered_detail(
                    endpoint,
                    id_arg,
                    id_fields,
                    filter_field,
                    filter_value,
                    &args,
                )
                .await
            }
            ToolKind::LookupIp => self.lookup_ip(&args).await,
            ToolKind::Raw => self.raw(&args).await,
            ToolKind::ClientRecord => self.client_record(&args).await,
            ToolKind::ClientBandwidth => self.client_bandwidth(&args).await,
            ToolKind::V2List {
                endpoint,
                output_key,
            } => self.v2_list(endpoint, output_key, &args).await,
            ToolKind::V2Detail { endpoint, id_arg } => {
                self.v2_detail(endpoint, id_arg, &args).await
            }
            ToolKind::V2NestedDetail {
                endpoint,
                id_arg,
                suffix,
            } => self.v2_nested_detail(endpoint, id_arg, suffix, &args).await,
            ToolKind::IntegrationList {
                endpoint,
                output_key,
            } => self.integration_list(endpoint, output_key, &args).await,
            ToolKind::IntegrationObject { endpoint } => {
                self.unifi
                    .integration_global_request(Method::GET, endpoint, 1, 0)
                    .await
            }
            ToolKind::IntegrationQuery { endpoint } => {
                self.integration_query(endpoint, &args).await
            }
            ToolKind::IntegrationWrite {
                method,
                endpoint,
                id_arg,
                body_required,
            } => {
                self.integration_write(method, endpoint, id_arg, body_required, &args)
                    .await
            }
            ToolKind::LegacyWlanUpdate => self.legacy_wlan_update(&args).await,
            ToolKind::Action {
                endpoint,
                command,
                id_arg,
                destructive,
                idempotent,
            } => {
                self.action(endpoint, command, id_arg, destructive, idempotent, &args)
                    .await
            }
            ToolKind::Compatibility {
                endpoint,
                method,
                id_arg,
                read_only,
                destructive,
                idempotent,
            } => {
                self.compatibility(
                    endpoint,
                    method,
                    id_arg,
                    read_only,
                    destructive,
                    idempotent,
                    &args,
                )
                .await
            }
        }
    }

    fn tool_index(&self, args: &Map<String, Value>) -> Value {
        let category = optional_string(args, "category").map(str::to_ascii_lowercase);
        let search = optional_string(args, "search").map(str::to_ascii_lowercase);
        let tools = tools::iter()
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

    async fn filtered_list(
        &self,
        endpoint: &str,
        output_key: &str,
        filter_field: &str,
        filter_value: &str,
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
        rows.retain(|row| row.get(filter_field).and_then(Value::as_str) == Some(filter_value));
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

    async fn filtered_detail(
        &self,
        endpoint: &str,
        id_arg: &str,
        id_fields: &[&str],
        filter_field: &str,
        filter_value: &str,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        let rows = extract_rows_owned(
            self.unifi
                .network_request(Method::GET, endpoint, Some(json!({"_limit":MAX_LIMIT})))
                .await?,
        );
        rows.into_iter()
            .filter(|row| row.get(filter_field).and_then(Value::as_str) == Some(filter_value))
            .find(|row| {
                id_fields.iter().any(|field| {
                    row.get(*field)
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.eq_ignore_ascii_case(identifier))
                })
            })
            .with_context(|| format!("No resource matched {id_arg} '{identifier}'"))
    }

    async fn site_manager_dashboard(&self, local_error: &anyhow::Error) -> Result<Value> {
        let client = self
            .unifi
            .site_manager
            .as_ref()
            .context("{local_error}; configure UNIFI_SITE_MANAGER_API_KEY_FILE for the cloud dashboard fallback")?;
        let site_id = client.resolve_site_id(&self.unifi.site).await?;
        let metrics = client.isp_metrics("5m", "24h").await?;
        Ok(
            json!({"source":"site_manager","site_id":site_id,"history_seconds":86400,"isp_metrics":metrics,"local_controller_error":local_error.to_string()}),
        )
    }

    async fn local_dashboard(
        &self,
        history_seconds: u64,
        aggregate_error: &anyhow::Error,
    ) -> Result<Value> {
        let health = self.dashboard_component("stat/health", None).await;
        let devices = self
            .dashboard_component("stat/device", Some(json!({"_limit":MAX_LIMIT})))
            .await;
        let clients = self
            .dashboard_component("stat/sta", Some(json!({"_limit":MAX_LIMIT})))
            .await;
        let wlans = self
            .dashboard_component("list/wlanconf", Some(json!({"_limit":MAX_LIMIT})))
            .await;
        let alarms = self
            .dashboard_component("stat/alarm", Some(json!({"_limit":MAX_LIMIT})))
            .await;
        let events = self
            .dashboard_component("stat/event", Some(json!({"_limit":MAX_LIMIT})))
            .await;
        Ok(json!({
            "source": "local_aggregate",
            "site": self.unifi.site,
            "history_seconds": history_seconds,
            "health": health,
            "devices": devices,
            "clients": clients,
            "wlans": wlans,
            "alarms": alarms,
            "events": events,
            "aggregated_route_error": aggregate_error.to_string(),
        }))
    }

    async fn dashboard_component(&self, endpoint: &str, body: Option<Value>) -> Value {
        match self
            .unifi
            .network_request(Method::GET, endpoint, body)
            .await
        {
            Ok(value) => value,
            Err(error) => json!({"error": error.to_string(), "endpoint": endpoint}),
        }
    }

    async fn special_read(
        &self,
        operation: SpecialReadOperation,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        match operation {
            SpecialReadOperation::BatchStatus => bail!(
                "Batch status has no supported route on this controller; cmd/batch-status is not exposed"
            ),
            SpecialReadOperation::Dashboard => {
                let history = args
                    .get("history_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(86400);
                if self.unifi.site_manager.is_some()
                    && self.unifi.api_key.is_none()
                    && self.unifi.username.is_none()
                {
                    return self
                        .site_manager_dashboard(&anyhow::anyhow!(
                            "local controller transport is not configured"
                        ))
                        .await;
                }
                match self
                    .unifi
                    .integration_or_v2_request(
                        Method::GET,
                        &format!("aggregated-dashboard?historySeconds={history}"),
                        None,
                    )
                    .await
                {
                    Ok(value) => Ok(value),
                    Err(local_error) => match self.site_manager_dashboard(&local_error).await {
                        Ok(value) => Ok(value),
                        Err(_cloud_error) => {
                            tracing::debug!(
                                "cloud dashboard fallback unavailable; aggregating supported local reads"
                            );
                            self.local_dashboard(history, &local_error).await
                        }
                    },
                }
            }
            SpecialReadOperation::RecentEvents => Ok(
                json!({"events":[],"count":0,"listening":false,"attached":false,"buffer_size":0,"buffer_capacity":0,"hint":"The Rust/stdin server does not run the upstream websocket listener; use unifi_list_events for historical events."}),
            ),
            SpecialReadOperation::SubscribeEvents => Ok(
                json!({"success":true,"resource_uri":"unifi://network/events","summary_uri":"unifi://network/events/recent","listening":false,"attached":false,"buffer_size":0,"buffer_capacity":0,"instructions":"The Rust/stdin server does not run the upstream websocket listener; use unifi_list_events for historical events."}),
            ),
            SpecialReadOperation::Events | SpecialReadOperation::EventTypes => {
                let limit = bounded_limit(
                    args.get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                );
                let start = bounded_offset(args.get("start").and_then(Value::as_u64));
                let within = args
                    .get("within_hours")
                    .and_then(Value::as_u64)
                    .unwrap_or(24);
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                let mut body = json!({"timestampFrom":now-(within as i64*3_600_000),"timestampTo":now,"severities":["LOW","MEDIUM","HIGH","VERY_HIGH"],"categories":["CLIENT_DEVICES","INTERNET_AND_WAN","POWER","SECURITY","UNIFI_DEVICES","SOFTWARE_UPDATES","UNIFI_ETHERNET_PORTS","VPN"],"type":"GENERAL","pageNumber":start/limit,"pageSize":limit.min(100),"searchText":""});
                if let Some(value) = args.get("event_type") {
                    body["keys"] = json!([value]);
                }
                let payload = self
                    .unifi
                    .integration_or_v2_request(Method::POST, "system-log/all", Some(body))
                    .await?;
                if matches!(operation, SpecialReadOperation::EventTypes) {
                    let mut counts = std::collections::BTreeMap::<String, usize>::new();
                    for row in extract_rows_owned(payload) {
                        if let Some(key) = row
                            .get("key")
                            .or_else(|| row.get("event"))
                            .and_then(Value::as_str)
                        {
                            *counts.entry(key.to_owned()).or_default() += 1;
                        }
                    }
                    return Ok(
                        json!({"event_types":counts.into_iter().map(|(key,count)|json!({"key":key,"prefix":key,"count":count})).collect::<Vec<_>>() }),
                    );
                }
                Ok(payload)
            }
            SpecialReadOperation::Alarms => {
                let limit = bounded_limit(
                    args.get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                );
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                self.unifi.integration_or_v2_request(Method::POST, "system-log/critical", Some(json!({"timestampFrom":now-30*24*3_600_000,"timestampTo":now,"severities":["HIGH","VERY_HIGH"],"categories":["CLIENT_DEVICES","INTERNET_AND_WAN","POWER","SECURITY","UNIFI_DEVICES","SOFTWARE_UPDATES","UNIFI_ETHERNET_PORTS","VPN"],"type":"GENERAL","pageNumber":0,"pageSize":limit.min(100),"searchText":""}))).await
            }
            SpecialReadOperation::IpsEvents => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                self.unifi
                    .integration_or_v2_request(
                        Method::POST,
                        "system-log/all",
                        Some(json!({
                            "timestampFrom": now - 24 * 3_600_000,
                            "timestampTo": now,
                            "severities": ["LOW", "MEDIUM", "HIGH", "VERY_HIGH"],
                            "categories": ["SECURITY"],
                            "type": "GENERAL",
                            "pageNumber": 0,
                            "pageSize": MAX_LIMIT.min(100),
                            "searchText": ""
                        })),
                    )
                    .await
            }
            SpecialReadOperation::SpeedtestResults => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                self.unifi.network_request(Method::POST,"stat/report/archive.speedtest",Some(json!({"attrs":["xput_download","xput_upload","latency","time"],"start":now-24*3_600_000,"end":now}))).await
            }
            SpecialReadOperation::TrafficFlows => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                self.unifi.integration_or_v2_request(Method::POST,"traffic-flows",Some(json!({"time_from":args.get("time_from").cloned().unwrap_or(json!(now-24*3_600_000)),"time_to":args.get("time_to").cloned().unwrap_or(json!(now)),"page_number":args.get("page").cloned().unwrap_or(json!(0)),"page_size":args.get("page_size").cloned().unwrap_or(json!(100)),"search_text":args.get("search_text").cloned().unwrap_or(json!("")),"skip_count":false}))).await
            }
            SpecialReadOperation::TrafficFlowStatistics => {
                let period = args.get("period").and_then(Value::as_str).unwrap_or("DAY");
                let top = args
                    .get("top")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 100);
                self.unifi
                    .integration_or_v2_request(
                        Method::GET,
                        &format!("traffic-flow-latest-statistics?period={period}&top={top}"),
                        None,
                    )
                    .await
            }
            SpecialReadOperation::Backups => {
                self.unifi
                    .network_request(
                        Method::POST,
                        "cmd/backup",
                        Some(json!({"cmd":"list-backups"})),
                    )
                    .await
            }
        }
    }

    async fn integration_write(
        &self,
        method: IntegrationMethod,
        endpoint_template: &str,
        id_arg: Option<&str>,
        body_required: bool,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let mut endpoint = endpoint_template.to_owned();
        let identifier = if let Some(key) = id_arg {
            let value = required_string(args, key)?;
            endpoint = endpoint.replace("{id}", value);
            Some(value.to_owned())
        } else {
            None
        };
        if endpoint.contains("{port}") {
            let port = args
                .get("port_index")
                .and_then(Value::as_u64)
                .context("port_index must be an integer")?;
            endpoint = endpoint.replace("{port}", &port.to_string());
        }
        let body = args.get("body").cloned();
        if body_required && body.as_ref().and_then(Value::as_object).is_none() {
            bail!("body must be an object for this Integration API operation");
        }
        let method_name = match method {
            IntegrationMethod::Post => "POST",
            IntegrationMethod::Put => "PUT",
            IntegrationMethod::Patch => "PATCH",
            IntegrationMethod::Delete => "DELETE",
        };
        let preview = json!({
            "method": method_name,
            "endpoint": endpoint,
            "target": identifier,
            "body": body,
            "query": args.get("query"),
            "destructive": true,
            "requires_confirmation": true,
        });
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(json!({"preview":preview,"requires_confirmation":true}));
        }
        let request_method = match method {
            IntegrationMethod::Post => Method::POST,
            IntegrationMethod::Put => Method::PUT,
            IntegrationMethod::Patch => Method::PATCH,
            IntegrationMethod::Delete => Method::DELETE,
        };
        let query = args
            .get("query")
            .and_then(Value::as_object)
            .map(|query| {
                query
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            value
                                .as_str()
                                .map(ToOwned::to_owned)
                                .unwrap_or_else(|| value.to_string()),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let query = query
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>();
        let data = self
            .unifi
            .integration_request_with_query(request_method, &endpoint, body, &query)
            .await?;
        Ok(json!({"preview":preview,"confirmed":true,"data":data}))
    }

    async fn legacy_wlan_update(&self, args: &Map<String, Value>) -> Result<Value> {
        legacy::wlan::update(self, args).await
    }

    async fn v2_list(
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
        let offset = bounded_offset(args.get("offset").and_then(Value::as_u64));
        let payload = self
            .unifi
            .integration_or_v2_request_with_query(
                Method::GET,
                endpoint,
                None,
                &[("limit", limit.to_string()), ("offset", offset.to_string())],
            )
            .await?;
        let reported_total = payload
            .get("totalCount")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let mut rows = extract_rows_owned(payload);
        if let Some(query) = optional_string(args, "query")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            rows.retain(|row| value_contains(row, query));
        }
        let total_count = reported_total.unwrap_or(rows.len());
        rows.truncate(limit);
        Ok(
            json!({"offset":offset,"limit":limit,"total_count":total_count,"returned_count":rows.len(),output_key:rows}),
        )
    }

    async fn v2_detail(
        &self,
        endpoint: &str,
        id_arg: &str,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        self.unifi
            .integration_or_v2_request(Method::GET, &format!("{endpoint}/{identifier}"), None)
            .await
    }

    async fn v2_nested_detail(
        &self,
        endpoint: &str,
        id_arg: &str,
        suffix: &str,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        self.unifi
            .integration_or_v2_request(
                Method::GET,
                &format!("{endpoint}/{identifier}/{suffix}"),
                None,
            )
            .await
    }

    async fn integration_query(&self, endpoint: &str, args: &Map<String, Value>) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(Value::as_object)
            .context("query must be an object")?;
        let pairs = query
            .iter()
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(value.to_string()))
                    .unwrap_or_default();
                (key.clone(), value)
            })
            .collect::<Vec<_>>();
        let pairs = pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>();
        self.unifi
            .integration_request_with_query(Method::GET, endpoint, None, &pairs)
            .await
    }

    async fn integration_list(
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
        let offset = bounded_offset(args.get("offset").and_then(Value::as_u64));
        let payload = self
            .unifi
            .integration_global_request(Method::GET, endpoint, limit, offset)
            .await?;
        let reported_total = payload
            .get("totalCount")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let mut rows = extract_rows_owned(payload);
        let query = optional_string(args, "query")
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if let Some(query) = query {
            rows.retain(|row| value_contains(row, query));
        }
        let total_count = if query.is_some() {
            rows.len()
        } else {
            reported_total.unwrap_or(rows.len())
        };
        rows.truncate(limit);
        Ok(
            json!({"offset":offset,"limit":limit,"total_count":total_count,"returned_count":rows.len(),output_key:rows}),
        )
    }

    async fn action(
        &self,
        endpoint: &str,
        command: &str,
        id_arg: &str,
        destructive: bool,
        idempotent: bool,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        let payload = json!({"mac": identifier, "cmd": command});
        let preview = json!({"endpoint": endpoint, "command": command, "target": {id_arg: identifier}, "destructive": destructive, "idempotent": idempotent});
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(json!({"requires_confirmation": true, "preview": preview}));
        }
        let response = self
            .unifi
            .network_request(Method::POST, endpoint, Some(payload))
            .await?;
        Ok(
            json!({"applied": true, "preview": preview, "controller_response": truncate_payload(response, 10)}),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn compatibility(
        &self,
        endpoint_template: &str,
        method: CompatibilityMethod,
        id_arg: Option<&str>,
        read_only: bool,
        destructive: bool,
        idempotent: bool,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        if read_only && endpoint_template == "special:batch-status" {
            return self
                .special_read(SpecialReadOperation::BatchStatus, args)
                .await;
        }
        if read_only && endpoint_template == "special:subscribe-events" {
            return self
                .special_read(SpecialReadOperation::SubscribeEvents, args)
                .await;
        }
        let integration = endpoint_template.strip_prefix("v2:").is_some();
        let endpoint_template = endpoint_template
            .strip_prefix("v2:")
            .unwrap_or(endpoint_template);
        let mut endpoint = endpoint_template.to_owned();
        let identifier = id_arg
            .map(|key| required_compatibility_identifier(args, key))
            .transpose()?;
        if let Some(identifier) = identifier {
            endpoint = endpoint.replace("{id}", identifier);
        }
        let request_method = match method {
            CompatibilityMethod::Get => Method::GET,
            CompatibilityMethod::Post => Method::POST,
            CompatibilityMethod::Put => Method::PUT,
            CompatibilityMethod::Delete => Method::DELETE,
        };
        if read_only {
            return if integration {
                self.unifi
                    .integration_or_v2_request(
                        request_method,
                        &endpoint,
                        Some(json!({"_limit": MAX_LIMIT})),
                    )
                    .await
            } else {
                self.unifi
                    .network_request(
                        request_method,
                        &endpoint,
                        Some(json!({"_limit": MAX_LIMIT})),
                    )
                    .await
            };
        }
        let body = compatibility_payload(args);
        let mut preview_body = body.clone();
        redact_sensitive(&mut preview_body);
        let preview = json!({
            "method": request_method.as_str(),
            "endpoint": endpoint,
            "target": id_arg.map(|key| json!({key: identifier})),
            "body": preview_body,
            "destructive": destructive,
            "idempotent": idempotent,
            "requires_confirmation": true,
        });
        if !args
            .get("confirm")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(json!({"preview": preview, "requires_confirmation": true}));
        }

        let merge_write = matches!(method, CompatibilityMethod::Put)
            && id_arg.is_some()
            && endpoint_template.contains("{id}");
        let request_body = if merge_write {
            let current = if integration {
                self.unifi
                    .integration_or_v2_request(Method::GET, &endpoint, None)
                    .await?
            } else {
                self.unifi
                    .network_request(Method::GET, &endpoint, None)
                    .await?
            };
            let mut merged = Value::Object(write_object_from_response(current)?);
            deep_merge(&mut merged, &body);
            merged
        } else {
            body.clone()
        };
        let response = if integration {
            self.unifi
                .integration_or_v2_request(request_method, &endpoint, Some(request_body))
                .await?
        } else {
            self.unifi
                .network_request(request_method, &endpoint, Some(request_body))
                .await?
        };

        let verification = if merge_write {
            let readback = if integration {
                self.unifi
                    .integration_or_v2_request(Method::GET, &endpoint, None)
                    .await?
            } else {
                self.unifi
                    .network_request(Method::GET, &endpoint, None)
                    .await?
            };
            let actual = write_object_from_response(readback)?;
            let mismatches = value_mismatches(&body, &actual, "");
            json!({
                "status": if mismatches.is_empty() { "verified" } else { "mismatch" },
                "mismatches": mismatches,
            })
        } else {
            json!({
                "status": "not_available",
                "reason": "This legacy command has no stable read-back route; the controller response is returned without treating HTTP success as proof of applied state.",
            })
        };
        Ok(json!({
            "confirmed": true,
            "preview": preview,
            "controller_response": truncate_payload(response, 10),
            "verification": verification,
        }))
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
            tools: tools::iter().map(tool_model).collect(),
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
        let site_manager_api_key =
            optional_secret_env_pair("UNIFI_SITE_MANAGER_API_KEY", "UNIFI_CLOUD_API_KEY")?;
        let base_url = match resolve_base_url() {
            Ok(url) => url,
            Err(_error) if site_manager_api_key.is_some() => {
                Url::parse("https://127.0.0.1").context("invalid cloud-only fallback URL")?
            }
            Err(error) => return Err(error),
        };
        let site =
            env_pair("UNIFI_NETWORK_SITE", "UNIFI_SITE").unwrap_or_else(|| DEFAULT_SITE.into());
        let api_key = optional_secret_env_pair("UNIFI_NETWORK_API_KEY", "UNIFI_API_KEY")?;
        let username = env_pair("UNIFI_NETWORK_USERNAME", "UNIFI_USERNAME");
        let password = optional_secret_env_pair("UNIFI_NETWORK_PASSWORD", "UNIFI_PASSWORD")?;
        if api_key.is_none()
            && (username.is_none() || password.is_none())
            && site_manager_api_key.is_none()
        {
            bail!(
                "configure a local API key, local username/password, or UNIFI_SITE_MANAGER_API_KEY_FILE"
            );
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
        let site_manager_api_key =
            optional_secret_env_pair("UNIFI_SITE_MANAGER_API_KEY", "UNIFI_CLOUD_API_KEY")?;
        let site_manager_site_id = env_pair("UNIFI_SITE_MANAGER_SITE_ID", "UNIFI_CLOUD_SITE_ID");
        let site_manager_console_id =
            env_pair("UNIFI_SITE_MANAGER_CONSOLE_ID", "UNIFI_CLOUD_CONSOLE_ID");
        Ok(Self {
            base_url,
            site,
            api_key,
            username,
            password,
            insecure_tls,
            redact_sensitive_fields,
            site_manager_api_key,
            site_manager_site_id,
            site_manager_console_id,
        })
    }
}

fn special_read_schema(operation: SpecialReadOperation) -> JsonObject {
    match operation {
        SpecialReadOperation::Events => schema(
            json!({"within_hours":{"type":"integer","minimum":0,"default":24},"limit":{"type":"integer","minimum":0,"maximum":500,"default":100},"start":{"type":"integer","minimum":0,"default":0},"event_type":{"type":"string"},"categories":{"type":"array","items":{"type":"string"}},"severities":{"type":"array","items":{"type":"string"}}}),
            &[],
        ),
        SpecialReadOperation::Alarms => schema(
            json!({"include_archived":{"type":"boolean","default":false},"limit":{"type":"integer","minimum":1,"maximum":500,"default":100}}),
            &[],
        ),
        SpecialReadOperation::RecentEvents => schema(
            json!({"event_type":{"type":"string"},"mac":{"type":"string"},"limit":{"type":"integer","minimum":0}}),
            &[],
        ),
        SpecialReadOperation::TrafficFlows => schema(
            json!({"within_hours":{"type":"integer","minimum":1,"default":24},"time_from":{"type":"integer"},"time_to":{"type":"integer"},"page":{"type":"integer","minimum":0,"default":0},"page_size":{"type":"integer","minimum":1,"maximum":1000,"default":100},"search_text":{"type":"string"}}),
            &[],
        ),
        SpecialReadOperation::TrafficFlowStatistics => schema(
            json!({"period":{"type":"string","enum":["HOUR","DAY","WEEK","MONTH"],"default":"DAY"},"top":{"type":"integer","minimum":1,"maximum":100,"default":10}}),
            &[],
        ),
        _ => schema(json!({}), &[]),
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
        ToolKind::Detail { id_arg, .. } | ToolKind::FilteredDetail { id_arg, .. } => {
            schema(json!({id_arg:{"type":"string"}}), &[id_arg])
        }
        ToolKind::LookupIp => schema(json!({"ip_address":{"type":"string"}}), &["ip_address"]),
        ToolKind::ClientRecord => schema(
            json!({"mac":{"type":"string","description":"Client MAC address."}}),
            &["mac"],
        ),
        ToolKind::ClientBandwidth => schema(
            json!({
                "mac":{"type":"string","description":"Client MAC address."},
                "interval":{"type":"string","enum":["5minutes","hourly","daily"],"default":"5minutes"},
                "start":{"type":"integer","minimum":0,"description":"Unix timestamp; defaults to seven days before end."},
                "end":{"type":"integer","minimum":1,"description":"Unix timestamp; defaults to now."},
                "attrs":{"type":"array","items":{"type":"string"},"description":"Optional controller report attributes; defaults to bytes-rx and bytes-tx."}
            }),
            &["mac"],
        ),
        ToolKind::Raw => schema(
            json!({"endpoint":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":500}}),
            &["endpoint"],
        ),
        ToolKind::List {
            endpoint: "stat/sitedpi",
            ..
        } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"query":{"type":"string"},"mac":{"type":"string","description":"Optional client MAC address for per-client DPI."},"summary":{"type":"boolean","description":"Return compact records. Defaults to true; set false only when the full selected controller record is required."}}),
            &[],
        ),
        ToolKind::List { .. } | ToolKind::FilteredList { .. } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"query":{"type":"string"},"summary":{"type":"boolean","description":"Return compact records. Defaults to true; set false only when the full selected controller record is required."}}),
            &[],
        ),
        ToolKind::V2List { .. } | ToolKind::IntegrationList { .. } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"offset":{"type":"integer","minimum":0},"query":{"type":"string"}}),
            &[],
        ),
        ToolKind::IntegrationObject { .. } => schema(json!({}), &[]),
        ToolKind::IntegrationQuery { .. } => schema(
            json!({"query":{"type":"object","description":"Query parameters required by the official endpoint."}}),
            &["query"],
        ),
        ToolKind::IntegrationWrite {
            id_arg,
            endpoint,
            body_required,
            ..
        } => {
            let mut properties = json!({
                "body": {"type":"object","description":"Request body matching the official UniFi Network API schema."},
                "confirm": {"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."},
                "query": {"type":"object","description":"Optional query parameters required by the official endpoint."},
                "port_index": {"type":"integer","minimum":0}
            });
            if let Some(id_arg) = id_arg {
                properties[id_arg] = json!({"type":"string"});
            }
            let mut required = Vec::new();
            if let Some(id_arg) = id_arg {
                required.push(id_arg);
            }
            if body_required {
                required.push("body");
            }
            if endpoint.contains("{port}") {
                required.push("port_index");
            }
            schema(properties, &required)
        }
        ToolKind::LegacyWlanUpdate => schema(
            json!({
                "body": {"type":"object","description":"WLAN fields to update; current fields are preserved."},
                "confirm": {"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."},
                "wifi_broadcast_id": {"type":"string"}
            }),
            &["wifi_broadcast_id", "body"],
        ),
        ToolKind::V2Detail { id_arg, .. } | ToolKind::V2NestedDetail { id_arg, .. } => {
            schema(json!({id_arg:{"type":"string"}}), &[id_arg])
        }
        ToolKind::SpecialRead { operation } => special_read_schema(operation),
        ToolKind::Dashboard => schema(json!({}), &[]),
        ToolKind::Action { id_arg, .. } => schema(
            json!({id_arg:{"type":"string"},"confirm":{"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."}}),
            &[id_arg],
        ),
        ToolKind::Compatibility {
            id_arg, read_only, ..
        } => {
            let mut properties = json!({
                "body": {"type":"object","description":"Controller fields for this compatibility operation."},
                "confirm": {"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."}
            });
            if let Some(id_arg) = id_arg {
                properties[id_arg] = json!({"type":"string"});
            }
            let required = if read_only {
                Vec::new()
            } else {
                id_arg.into_iter().collect()
            };
            schema(properties, &required)
        }
    };
    let manifest = manifest_tool(spec.name);
    let input_schema = manifest
        .and_then(|tool| tool.get("schema"))
        .and_then(|schema| schema.get("input"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or(input_schema);
    let title = manifest
        .and_then(|tool| tool.get("title"))
        .and_then(Value::as_str)
        .unwrap_or(spec.title);
    let description = manifest
        .and_then(|tool| tool.get("description"))
        .and_then(Value::as_str)
        .unwrap_or(spec.description);
    let default_read_only = !matches!(
        spec.kind,
        ToolKind::Action { .. }
            | ToolKind::IntegrationWrite { .. }
            | ToolKind::LegacyWlanUpdate
            | ToolKind::Compatibility {
                read_only: false,
                ..
            }
    );
    let default_destructive = matches!(
        spec.kind,
        ToolKind::Action {
            destructive: true,
            ..
        } | ToolKind::IntegrationWrite { .. }
            | ToolKind::LegacyWlanUpdate
            | ToolKind::Compatibility {
                destructive: true,
                ..
            }
    );
    let default_idempotent = match spec.kind {
        ToolKind::Action { idempotent, .. } | ToolKind::Compatibility { idempotent, .. } => {
            idempotent
        }
        ToolKind::IntegrationWrite { .. } | ToolKind::LegacyWlanUpdate => false,
        _ => true,
    };
    let read_only = manifest.map_or(Some(default_read_only), |_| {
        manifest_hint(manifest, "readOnlyHint").flatten()
    });
    let destructive = manifest.map_or(Some(default_destructive), |_| {
        manifest_hint(manifest, "destructiveHint").flatten()
    });
    let idempotent = manifest.map_or(Some(default_idempotent), |_| {
        manifest_hint(manifest, "idempotentHint").flatten()
    });
    let mut annotations = ToolAnnotations::with_title(title).open_world(false);
    if let Some(value) = read_only {
        annotations = annotations.read_only(value);
    }
    if let Some(value) = destructive {
        annotations = annotations.destructive(value);
    }
    if let Some(value) = idempotent {
        annotations = annotations.idempotent(value);
    }
    Tool::new(
        Cow::Borrowed(spec.name),
        Cow::Borrowed(description),
        Arc::new(input_schema),
    )
    .with_title(title)
    .with_annotations(annotations)
}
fn required_compatibility_identifier<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str> {
    let value = required_string(args, key)?;
    if key.contains("mac") {
        required_mac(args, key)?;
    } else if value.contains('/') || value.contains('\\') || value == "." || value == ".." {
        bail!("{key} must be a safe controller identifier");
    }
    Ok(value)
}

fn compatibility_payload(args: &Map<String, Value>) -> Value {
    for key in [
        "body",
        "group_data",
        "record_data",
        "entry_data",
        "policy_data",
        "port_forward_data",
        "qos_data",
        "rule",
        "update_data",
        "filter_data",
        "radio",
        "port_overrides",
    ] {
        if let Some(value) = args.get(key) {
            return value.clone();
        }
    }
    let mut object = args.clone();
    object.remove("confirm");
    Value::Object(object)
}

fn write_object_from_response(response: Value) -> Result<Map<String, Value>> {
    let payload = response.get("data").unwrap_or(&response);
    payload
        .as_object()
        .cloned()
        .context("compatibility write verification expected an object response")
}

fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = base.get_mut(key) {
                    deep_merge(existing, value);
                } else {
                    base.insert(key.clone(), value.clone());
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

fn value_mismatches(expected: &Value, actual: &Map<String, Value>, path: &str) -> Vec<Value> {
    let mut mismatches = Vec::new();
    let Some(expected) = expected.as_object() else {
        return mismatches;
    };
    for (key, expected_value) in expected {
        let location = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        match actual.get(key) {
            Some(Value::Object(actual_object)) if expected_value.is_object() => {
                mismatches.extend(value_mismatches(expected_value, actual_object, &location));
            }
            Some(actual_value) if actual_value == expected_value => {}
            Some(actual_value) => mismatches.push(
                json!({"field": location, "expected": expected_value, "actual": actual_value}),
            ),
            None => mismatches
                .push(json!({"field": location, "expected": expected_value, "actual": null})),
        }
    }
    mismatches
}

fn manifest_tool(name: &str) -> Option<&'static Value> {
    static MANIFEST: OnceLock<Value> = OnceLock::new();
    MANIFEST
        .get_or_init(|| {
            serde_json::from_str(include_str!("../upstream_network_tools_manifest.json"))
                .expect("checked-in upstream Network manifest must be valid JSON")
        })
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
        })
}

fn manifest_hint(tool: Option<&Value>, key: &str) -> Option<Option<bool>> {
    tool.and_then(|tool| tool.get("annotations"))
        .and_then(|annotations| annotations.get(key))
        .map(Value::as_bool)
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
fn required_mac<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    let mac = required_string(args, key)?;
    let valid = mac.len() == 17
        && mac.chars().enumerate().all(|(index, ch)| {
            if index % 3 == 2 {
                ch == ':'
            } else {
                ch.is_ascii_hexdigit()
            }
        });
    if !valid {
        bail!("{key} must be a colon-separated MAC address");
    }
    Ok(mac)
}
fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
fn optional_string<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn integration_site_id_from_payload(payload: &Value, legacy_site: &str) -> Result<String> {
    let sites = payload
        .get("data")
        .and_then(Value::as_array)
        .context("Integration site discovery returned no site list")?;
    let site = sites
        .iter()
        .find(|site| site.get("internalReference").and_then(Value::as_str) == Some(legacy_site))
        .with_context(|| {
            format!("Integration site discovery found no site matching legacy site '{legacy_site}'")
        })?;
    site.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .context("Integration site discovery returned a matching site without an ID")
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
fn bounded_offset(offset: Option<u64>) -> usize {
    offset.unwrap_or(0).min(usize::MAX as u64) as usize
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
        "rest/dynamicdns" => &["_id", "service", "host_name", "interface", "enabled"],
        "stat/voucher" => &[
            "_id",
            "code",
            "create_time",
            "duration",
            "quota",
            "used",
            "status",
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
    bail!("configure UNIFI_NETWORK_BASE_URL/UNIFI_BASE_URL or UNIFI_NETWORK_HOST/UNIFI_HOST")
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
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();
    let server = UnifiMcp::new(UnifiClient::new(UnifiConfig::from_env()?)?);
    if matches!(
        env_pair("UNIFI_MCP_TRANSPORT", "UNIFI_NETWORK_MCP_TRANSPORT").as_deref(),
        Some("http" | "streamable-http")
    ) {
        serve_http(server).await
    } else {
        let service = server.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

async fn serve_http(server: UnifiMcp) -> Result<()> {
    let token =
        optional_secret_env_pair("UNIFI_MCP_HTTP_TOKEN", "UNIFI_NETWORK_MCP_HTTP_TOKEN")?
            .context("HTTP transport requires UNIFI_MCP_HTTP_TOKEN_FILE or UNIFI_MCP_HTTP_TOKEN")?;
    let bind = env_pair("UNIFI_MCP_HTTP_BIND", "UNIFI_NETWORK_MCP_HTTP_BIND")
        .unwrap_or_else(|| "127.0.0.1:8000".into());
    let address: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid HTTP bind address: {bind}"))?;
    let allowed_host = env_pair(
        "UNIFI_MCP_HTTP_ALLOWED_HOST",
        "UNIFI_NETWORK_MCP_HTTP_ALLOWED_HOST",
    )
    .unwrap_or_else(|| address.to_string());
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts([allowed_host])
        .with_json_response(true);
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind MCP HTTP transport on {address}"))?;
    tracing::info!("MCP Streamable HTTP listening on {address}");

    loop {
        let (stream, peer) = listener.accept().await.context("MCP HTTP accept failed")?;
        let service = service.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let http_service = service_fn(move |request: Request<Incoming>| {
                let mut service = service.clone();
                let token = token.clone();
                async move {
                    let authorised = request
                        .headers()
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.strip_prefix("Bearer "))
                        .is_some_and(|value| value == token);
                    if !authorised {
                        tracing::warn!(target: "unifi_mcp::http", %peer, "unauthorised MCP HTTP request");
                        let body = Full::new(hyper::body::Bytes::from_static(b"unauthorized"))
                            .map_err(|never: Infallible| match never {})
                            .boxed();
                        return Ok::<_, Infallible>(
                            Response::builder()
                                .status(StatusCode::UNAUTHORIZED)
                                .header("www-authenticate", "Bearer")
                                .body(body)
                                .expect("valid unauthorized response"),
                        );
                    }
                    service.call(request).await
                }
            });
            if let Err(error) = http1::Builder::new()
                .serve_connection(io, http_service)
                .with_upgrades()
                .await
            {
                tracing::debug!("MCP HTTP connection closed: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_limits() {
        assert_eq!(bounded_limit(None), 100);
        assert_eq!(bounded_limit(Some(0)), 1);
        assert_eq!(bounded_limit(Some(501)), 500);
        assert_eq!(bounded_offset(None), 0);
        assert_eq!(bounded_offset(Some(25)), 25);
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
        let mut names = tools::iter().map(|t| t.name).collect::<Vec<_>>();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }
    #[test]
    fn client_history_tools_are_catalogued() {
        for name in ["unifi_get_client_record", "unifi_get_client_bandwidth"] {
            let spec = UnifiMcp::find_tool(name).expect("catalogued client history tool");
            assert!(spec.description.contains("client"));
            assert_eq!(
                tool_model(spec).annotations.unwrap().read_only_hint,
                Some(true)
            );
        }
    }
    #[test]
    fn client_mac_validation_rejects_path_injection() {
        let mut args = Map::new();
        args.insert("mac".into(), json!("../../rest/networkconf"));
        assert!(required_mac(&args, "mac").is_err());
        args.insert("mac".into(), json!("c0:d7:aa:b5:2a:06"));
        assert_eq!(required_mac(&args, "mac").unwrap(), "c0:d7:aa:b5:2a:06");
    }
    #[test]
    fn upstream_compatibility_manifest_is_reconciled() {
        let tools =
            serde_json::from_str::<Value>(include_str!("../upstream_network_tools_manifest.json"))
                .expect("manifest JSON")["tools"]
                .as_array()
                .cloned()
                .expect("manifest tools");
        assert_eq!(tools.len(), 194);
        for manifest in tools {
            let name = manifest["name"].as_str().expect("manifest name");
            let spec = UnifiMcp::find_tool(name).expect("manifest tool is catalogued");
            if !tools::is_compatibility(name) {
                continue;
            }
            let model = tool_model(spec);
            assert_eq!(
                model.input_schema.as_ref(),
                manifest["schema"]["input"]
                    .as_object()
                    .expect("manifest input schema")
            );
            let annotations = model.annotations.expect("tool annotations");
            assert_eq!(
                annotations.read_only_hint,
                manifest["annotations"]["readOnlyHint"].as_bool(),
                "{name}"
            );
            assert_eq!(
                annotations.destructive_hint,
                manifest["annotations"]["destructiveHint"].as_bool(),
                "{name}"
            );
            assert_eq!(
                annotations.idempotent_hint,
                manifest["annotations"]["idempotentHint"].as_bool(),
                "{name}"
            );
        }
    }

    #[test]
    fn compatibility_merge_and_identifier_guards() {
        let mut current = json!({"name":"old","nested":{"keep":true,"change":1}});
        deep_merge(&mut current, &json!({"nested":{"change":2}}));
        assert_eq!(
            current,
            json!({"name":"old","nested":{"keep":true,"change":2}})
        );
        let mut args = Map::new();
        args.insert("id".into(), json!("../../rest/networkconf"));
        assert!(required_compatibility_identifier(&args, "id").is_err());
    }

    #[test]
    fn catalog_annotations_match_manifest_or_mutability() {
        for tool in tools::iter() {
            let model = tool_model(tool);
            let annotations = model.annotations.unwrap();
            if let Some(manifest) = manifest_tool(tool.name) {
                assert_eq!(
                    annotations.read_only_hint,
                    manifest["annotations"]["readOnlyHint"].as_bool(),
                    "{}",
                    tool.name
                );
                assert_eq!(
                    annotations.destructive_hint,
                    manifest["annotations"]["destructiveHint"].as_bool(),
                    "{}",
                    tool.name
                );
                assert_eq!(
                    annotations.idempotent_hint,
                    manifest["annotations"]["idempotentHint"].as_bool(),
                    "{}",
                    tool.name
                );
                continue;
            }
            let mutating = matches!(
                tool.kind,
                ToolKind::Action { .. }
                    | ToolKind::IntegrationWrite { .. }
                    | ToolKind::LegacyWlanUpdate
                    | ToolKind::Compatibility {
                        read_only: false,
                        ..
                    }
            );
            assert_eq!(annotations.read_only_hint, Some(!mutating), "{}", tool.name);
        }
    }
    #[test]
    fn official_network_switching_and_dns_surface_is_catalogued() {
        for name in [
            "unifi_create_legacy_firewall_rule",
            "unifi_update_legacy_network",
        ] {
            assert!(UnifiMcp::find_tool(name).is_some(), "missing {name}");
        }
        for name in [
            "unifi_list_switch_stacks",
            "unifi_get_switch_stack_details",
            "unifi_list_mc_lag_domains",
            "unifi_get_mc_lag_domain_details",
            "unifi_list_lags",
            "unifi_get_lag_details",
            "unifi_list_dns_policies",
            "unifi_get_dns_policy_details",
            "unifi_create_dns_policy",
            "unifi_update_dns_policy",
            "unifi_delete_dns_policy",
        ] {
            assert!(UnifiMcp::find_tool(name).is_some(), "missing {name}");
        }
    }
    #[test]
    fn integration_mutations_require_confirmation_and_preview_inputs() {
        for name in [
            "unifi_create_network",
            "unifi_update_dns_policy",
            "unifi_execute_api_port_action",
        ] {
            let spec = UnifiMcp::find_tool(name).expect("catalogued mutation");
            let model = tool_model(spec);
            let schema = model.input_schema.as_ref();
            assert_eq!(schema["properties"]["confirm"]["type"], "boolean");
            assert_eq!(schema["properties"]["confirm"]["type"], "boolean");
        }
    }
    #[test]
    fn integration_site_selection_uses_matching_legacy_site() {
        let payload = json!({"data":[
            {"id":"first-uuid","internalReference":"first"},
            {"id":"default-uuid","internalReference":"default"}
        ]});
        assert_eq!(
            integration_site_id_from_payload(&payload, "default").unwrap(),
            "default-uuid"
        );
    }
    #[test]
    fn integration_site_selection_never_falls_back_to_another_site() {
        let payload = json!({"data":[{"id":"first-uuid","internalReference":"first"}]});
        let error = integration_site_id_from_payload(&payload, "default")
            .unwrap_err()
            .to_string();
        assert!(error.contains("no site matching legacy site 'default'"));
    }
}
