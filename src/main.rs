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
    V2List {
        endpoint: &'static str,
        output_key: &'static str,
    },
    V2Detail {
        endpoint: &'static str,
        id_arg: &'static str,
    },
    IntegrationList {
        endpoint: &'static str,
        output_key: &'static str,
    },
    IntegrationObject {
        endpoint: &'static str,
    },
    IntegrationWrite {
        method: IntegrationMethod,
        endpoint: &'static str,
        id_arg: Option<&'static str>,
        body_required: bool,
    },
    Action {
        endpoint: &'static str,
        command: &'static str,
        id_arg: &'static str,
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
    integration_object!(
        "unifi_get_api_application_info",
        "Official Network Application Info",
        "system",
        "v1/info"
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
                    if Self::find_tool(name).is_some_and(|tool| {
                        matches!(
                            tool.kind,
                            ToolKind::Action { .. } | ToolKind::IntegrationWrite { .. }
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
            ToolKind::V2List {
                endpoint,
                output_key,
            } => self.v2_list(endpoint, output_key, &args).await,
            ToolKind::V2Detail { endpoint, id_arg } => {
                self.v2_detail(endpoint, id_arg, &args).await
            }
            ToolKind::IntegrationList {
                endpoint,
                output_key,
            } => self.integration_list(endpoint, output_key, &args).await,
            ToolKind::IntegrationObject { endpoint } => {
                self.unifi
                    .integration_global_request(Method::GET, endpoint, 1)
                    .await
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
        let data = self
            .unifi
            .integration_request(request_method, &endpoint, body)
            .await?;
        Ok(json!({"preview":preview,"confirmed":true,"data":data}))
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
        let payload = self
            .unifi
            .integration_request_with_query(
                Method::GET,
                endpoint,
                None,
                &[("limit", limit.to_string()), ("offset", "0".to_owned())],
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
        Ok(json!({"total_count":total_count,"returned_count":rows.len(),output_key:rows}))
    }

    async fn v2_detail(
        &self,
        endpoint: &str,
        id_arg: &str,
        args: &Map<String, Value>,
    ) -> Result<Value> {
        let identifier = required_string(args, id_arg)?;
        self.unifi
            .integration_request(Method::GET, &format!("{endpoint}/{identifier}"), None)
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
        let payload = self
            .unifi
            .integration_global_request(Method::GET, endpoint, limit)
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
        Ok(json!({"total_count":total_count,"returned_count":rows.len(),output_key:rows}))
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
            integration_site_id: Arc::new(Mutex::new(None)),
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
    async fn integration_request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.integration_request_with_query(method, endpoint, body, &[])
            .await
    }
    async fn integration_request_with_query(
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
    async fn integration_global_request(
        &self,
        method: Method,
        endpoint: &str,
        limit: usize,
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
            .append_pair("offset", "0");
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
    async fn integration_site(&self) -> Result<String> {
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
        ToolKind::Raw => schema(
            json!({"endpoint":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":500}}),
            &["endpoint"],
        ),
        ToolKind::List { .. } | ToolKind::FilteredList { .. } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"query":{"type":"string"},"summary":{"type":"boolean","description":"Return compact records. Defaults to true; set false only when the full selected controller record is required."}}),
            &[],
        ),
        ToolKind::V2List { .. } | ToolKind::IntegrationList { .. } => schema(
            json!({"limit":{"type":"integer","minimum":1,"maximum":500},"query":{"type":"string"}}),
            &[],
        ),
        ToolKind::IntegrationObject { .. } => schema(json!({}), &[]),
        ToolKind::IntegrationWrite {
            id_arg,
            body_required,
            ..
        } => {
            let mut properties = json!({
                "body": {"type":"object","description":"Request body matching the official UniFi Network API schema."},
                "confirm": {"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."}
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
            schema(properties, &required)
        }
        ToolKind::V2Detail { id_arg, .. } => schema(json!({id_arg:{"type":"string"}}), &[id_arg]),
        ToolKind::Dashboard => schema(json!({}), &[]),
        ToolKind::Action { id_arg, .. } => schema(
            json!({id_arg:{"type":"string"},"confirm":{"type":"boolean","description":"Set true only after reviewing the preview. Defaults to false."}}),
            &[id_arg],
        ),
    };
    Tool::new(
        Cow::Borrowed(spec.name),
        Cow::Borrowed(spec.description),
        Arc::new(input_schema),
    )
    .with_title(spec.title)
    .with_annotations(
        ToolAnnotations::with_title(spec.title)
            .read_only(!matches!(
                spec.kind,
                ToolKind::Action { .. } | ToolKind::IntegrationWrite { .. }
            ))
            .destructive(matches!(
                spec.kind,
                ToolKind::Action {
                    destructive: true,
                    ..
                } | ToolKind::IntegrationWrite { .. }
            ))
            .idempotent(match spec.kind {
                ToolKind::Action { idempotent, .. } => idempotent,
                ToolKind::IntegrationWrite { .. } => false,
                _ => true,
            })
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
    fn catalog_annotations_match_mutability() {
        for tool in TOOLS {
            let model = tool_model(tool);
            let annotations = model.annotations.unwrap();
            let mutating = matches!(
                tool.kind,
                ToolKind::Action { .. } | ToolKind::IntegrationWrite { .. }
            );
            assert_eq!(annotations.read_only_hint, Some(!mutating));
            if let ToolKind::Action {
                destructive,
                idempotent,
                ..
            } = tool.kind
            {
                assert_eq!(annotations.destructive_hint, Some(destructive));
                assert_eq!(annotations.idempotent_hint, Some(idempotent));
            } else if matches!(tool.kind, ToolKind::IntegrationWrite { .. }) {
                assert_eq!(annotations.destructive_hint, Some(true));
                assert_eq!(annotations.idempotent_hint, Some(false));
            }
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
