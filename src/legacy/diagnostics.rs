use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde_json::{Map, Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    MAX_LIMIT, UnifiMcp, bounded_limit, compact_endpoint_rows, compact_rows, current_unix_seconds,
    extract_rows_owned, optional_string, required_mac, required_string, truncate_payload,
    value_contains,
};

impl UnifiMcp {
    pub(crate) async fn list(
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
        let dpi_mac = if endpoint == "stat/sitedpi" {
            if let Some(mac) = optional_string(args, "mac")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(mac.to_owned())
            } else if let Some(query) = optional_string(args, "query")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let clients = self
                    .unifi
                    .network_request(Method::GET, "stat/sta", Some(json!({"_limit":MAX_LIMIT})))
                    .await?;
                extract_rows_owned(clients).into_iter().find_map(|row| {
                    value_contains(&row, query)
                        .then(|| {
                            row.get("mac")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                        })
                        .flatten()
                })
            } else {
                None
            }
        } else {
            None
        };
        let payload = if endpoint == "stat/session" {
            let end = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock is before Unix epoch")?
                .as_secs();
            self.unifi
                .network_request(
                    Method::POST,
                    endpoint,
                    Some(json!({
                        "type": "all",
                        "start": end.saturating_sub(7 * 24 * 60 * 60),
                        "end": end
                    })),
                )
                .await?
        } else if endpoint == "stat/sitedpi" {
            let mut body = json!({"type":"by_app"});
            if let Some(mac) = dpi_mac.as_deref() {
                body["macs"] = json!([mac]);
            }
            self.unifi
                .network_request(Method::POST, endpoint, Some(body))
                .await?
        } else {
            self.unifi
                .network_request(Method::GET, endpoint, Some(json!({"_limit":MAX_LIMIT})))
                .await?
        };
        let fingerprint = if endpoint == "stat/sitedpi" {
            if let Some(mac) = dpi_mac.as_deref() {
                let clients = self
                    .unifi
                    .network_request(Method::GET, "stat/sta", Some(json!({"_limit":MAX_LIMIT})))
                    .await?;
                extract_rows_owned(clients).into_iter().find(|row| {
                    row.get("mac")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.eq_ignore_ascii_case(mac))
                })
            } else {
                None
            }
        } else {
            None
        };
        let mut rows = extract_rows_owned(payload);
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
        let mut result = json!({"site":self.unifi.site,"total_count":total_count,"returned_count":rows.len(),output_key:rows});
        if let Some(row) = fingerprint {
            let fields = [
                "mac",
                "hostname",
                "name",
                "device_name",
                "oui",
                "dev_vendor",
                "dev_cat",
                "dev_family",
                "os_name",
                "network",
                "network_id",
                "ip",
                "is_wired",
                "is_guest",
            ];
            result["fingerprint"] = Value::Object(
                fields
                    .iter()
                    .filter_map(|field| {
                        row.get(*field)
                            .map(|value| ((*field).into(), value.clone()))
                    })
                    .collect(),
            );
        }
        Ok(result)
    }

    pub(crate) async fn client_record(&self, args: &Map<String, Value>) -> Result<Value> {
        let mac = required_mac(args, "mac")?;
        let endpoint = format!("stat/user/{mac}");
        let payload = self
            .unifi
            .network_request(Method::GET, &endpoint, None)
            .await?;
        Ok(json!({"site": self.unifi.site, "mac": mac, "record": payload}))
    }

    pub(crate) async fn client_bandwidth(&self, args: &Map<String, Value>) -> Result<Value> {
        let mac = required_mac(args, "mac")?;
        let interval = optional_string(args, "interval").unwrap_or("5minutes");
        if !matches!(interval, "5minutes" | "hourly" | "daily") {
            bail!("interval must be one of: 5minutes, hourly, daily");
        }
        let end = args
            .get("end")
            .and_then(Value::as_u64)
            .unwrap_or_else(current_unix_seconds);
        let start = args
            .get("start")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| end.saturating_sub(7 * 24 * 60 * 60));
        if start >= end {
            bail!("start must be earlier than end");
        }
        let attrs = match args.get("attrs") {
            None | Some(Value::Null) => json!(["bytes-rx", "bytes-tx"]),
            Some(Value::Array(values))
                if !values.is_empty() && values.iter().all(Value::is_string) =>
            {
                Value::Array(values.clone())
            }
            Some(_) => bail!("attrs must be a non-empty array of strings"),
        };
        let endpoint = format!("stat/report/{interval}.user");
        let payload = self
            .unifi
            .network_request(
                Method::POST,
                &endpoint,
                Some(json!({"attrs": attrs, "mac": mac, "start": start, "end": end})),
            )
            .await?;
        Ok(json!({
            "site": self.unifi.site,
            "mac": mac,
            "interval": interval,
            "start": start,
            "end": end,
            "report": payload
        }))
    }

    pub(crate) async fn lookup_ip(&self, args: &Map<String, Value>) -> Result<Value> {
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

    pub(crate) async fn dashboard(&self) -> Result<Value> {
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

    pub(crate) async fn raw(&self, args: &Map<String, Value>) -> Result<Value> {
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
