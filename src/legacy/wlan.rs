use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::{MAX_LIMIT, UnifiMcp, extract_rows_owned, redact_sensitive, required_string};

pub(crate) async fn update(mcp: &UnifiMcp, args: &Map<String, Value>) -> Result<Value> {
    let wlan_id = required_string(args, "wifi_broadcast_id")?;
    let updates = args
        .get("body")
        .and_then(Value::as_object)
        .context("body must be an object for WLAN updates")?;
    if updates.is_empty() {
        bail!("body must contain at least one WLAN field");
    }

    // The legacy controller API expects the complete WLAN object. Keep this
    // internal read unredacted so a masked passphrase is never sent back to
    // the controller as if it were the real secret.
    let current_payload = mcp
        .unifi
        .network_request_unredacted(
            Method::GET,
            "list/wlanconf",
            Some(json!({"_limit": MAX_LIMIT})),
        )
        .await?;
    let current = extract_rows_owned(current_payload)
        .into_iter()
        .find(|row| row.get("_id").and_then(Value::as_str) == Some(wlan_id))
        .with_context(|| format!("WLAN '{wlan_id}' was not found"))?;
    let mut merged = current.clone();
    merge_json_objects(&mut merged, &Value::Object(updates.clone()));

    let preview = json!({
        "method": "PUT",
        "endpoint": format!("rest/wlanconf/{wlan_id}"),
        "target": {"wifi_broadcast_id": wlan_id},
        "body": Value::Object(updates.clone()),
        "current": current,
        "destructive": true,
        "requires_confirmation": true,
    });
    let mut safe_preview = preview.clone();
    if mcp.unifi.redact_sensitive_fields {
        redact_sensitive(&mut safe_preview);
    }
    if !args
        .get("confirm")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(json!({"preview":safe_preview,"requires_confirmation":true}));
    }

    mcp.unifi
        .network_request(
            Method::PUT,
            &format!("rest/wlanconf/{wlan_id}"),
            Some(merged),
        )
        .await?;

    let after_payload = mcp
        .unifi
        .network_request_unredacted(
            Method::GET,
            "list/wlanconf",
            Some(json!({"_limit": MAX_LIMIT})),
        )
        .await?;
    let after = extract_rows_owned(after_payload)
        .into_iter()
        .find(|row| row.get("_id").and_then(Value::as_str) == Some(wlan_id))
        .with_context(|| format!("WLAN '{wlan_id}' disappeared after update"))?;
    let mismatches = updates
        .iter()
        .filter_map(|(key, requested)| (after.get(key) != Some(requested)).then_some(key.clone()))
        .collect::<Vec<_>>();
    let mut safe_after = after;
    if mcp.unifi.redact_sensitive_fields {
        redact_sensitive(&mut safe_after);
    }
    if !mismatches.is_empty() {
        bail!(
            "WLAN update was sent but these fields did not persist: {}",
            mismatches.join(", ")
        );
    }
    Ok(json!({
        "preview": safe_preview,
        "confirmed": true,
        "wlan_id": wlan_id,
        "updated_fields": updates.keys().collect::<Vec<_>>(),
        "verified": true,
        "details": safe_after,
    }))
}

fn merge_json_objects(target: &mut Value, updates: &Value) {
    let (Some(target), Some(updates)) = (target.as_object_mut(), updates.as_object()) else {
        return;
    };
    for (key, value) in updates {
        if let (Some(existing), Value::Object(_)) = (target.get_mut(key), value) {
            merge_json_objects(existing, value);
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}
