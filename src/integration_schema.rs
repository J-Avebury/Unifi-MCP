//! Request schemas from the pinned official Network v10.4.57 OpenAPI document.
use crate::IntegrationMethod;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::OnceLock};

fn resolve(value: &Value, document: &Value, inheritance: bool) -> Value {
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        return resolve(
            document
                .pointer(&reference[1..])
                .expect("local schema reference"),
            document,
            inheritance,
        );
    }
    if !inheritance
        && let Some(mapping) = value
            .pointer("/discriminator/mapping")
            .and_then(Value::as_object)
    {
        let property = value["discriminator"]["propertyName"].as_str().unwrap();
        let variants: Vec<_> = mapping
            .iter()
            .map(|(tag, reference)| {
                let mut variant = resolve(&json!({"$ref":reference}), document, true);
                variant["properties"][property] = json!({"const":tag,"type":"string"});
                variant
            })
            .collect();
        return json!({"oneOf":variants});
    }
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut result = serde_json::Map::new();
    // Flatten inheritance before closing object properties. Base discriminators
    // are suppressed only in allOf, so nested discriminators still apply.
    if let Some(parts) = value.get("allOf").and_then(Value::as_array) {
        for part in parts {
            let part = resolve(part, document, true);
            for (key, v) in part.as_object().unwrap() {
                match key.as_str() {
                    "properties" => {
                        let target = result.entry(key.clone()).or_insert(json!({}));
                        target
                            .as_object_mut()
                            .unwrap()
                            .extend(v.as_object().unwrap().clone());
                    }
                    "required" => {
                        let fields = result
                            .entry(key.clone())
                            .or_insert(json!([]))
                            .as_array_mut()
                            .unwrap();
                        for field in v.as_array().unwrap() {
                            if !fields.contains(field) {
                                fields.push(field.clone());
                            }
                        }
                    }
                    _ => {
                        result.insert(key.clone(), v.clone());
                    }
                }
            }
        }
    }
    for (key, v) in object {
        match key.as_str() {
            "allOf" | "discriminator" | "example" | "xml" => {}
            "properties" => {
                let fields = result
                    .entry(key.clone())
                    .or_insert(json!({}))
                    .as_object_mut()
                    .unwrap();
                for (name, field) in v.as_object().unwrap() {
                    fields.insert(name.clone(), resolve(field, document, false));
                }
            }
            "required" => {
                let fields = result
                    .entry(key.clone())
                    .or_insert(json!([]))
                    .as_array_mut()
                    .unwrap();
                for field in v.as_array().unwrap() {
                    if !fields.contains(field) {
                        fields.push(field.clone());
                    }
                }
            }
            "items" => {
                result.insert(key.clone(), resolve(v, document, false));
            }
            _ => {
                result.insert(key.clone(), v.clone());
            }
        }
    }
    if result.contains_key("properties") {
        result.insert("type".into(), json!("object"));
        result.insert("additionalProperties".into(), json!(false));
    }
    Value::Object(result)
}

fn normalized(path: &str) -> String {
    path.split('/')
        .map(|part| if part.starts_with('{') { "{}" } else { part })
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn body(method: IntegrationMethod, endpoint: &str) -> Option<Value> {
    static SCHEMAS: OnceLock<HashMap<String, Value>> = OnceLock::new();
    let schemas = SCHEMAS.get_or_init(|| {
        let document: Value =
            serde_json::from_str(include_str!("integration_openapi.json")).unwrap();
        let mut schemas = HashMap::new();
        for (path, operations) in document["paths"].as_object().unwrap() {
            for (method, operation) in operations.as_object().unwrap() {
                if let Some(schema) =
                    operation.pointer("/requestBody/content/application~1json/schema")
                {
                    let endpoint = path.strip_prefix("/v1/sites/{siteId}/").unwrap_or(path);
                    schemas.insert(
                        format!("{method}:{}", normalized(endpoint)),
                        resolve(schema, &document, false),
                    );
                }
            }
        }
        schemas
    });
    let method = match method {
        IntegrationMethod::Post => "post",
        IntegrationMethod::Put => "put",
        IntegrationMethod::Patch => "patch",
        IntegrationMethod::Delete => "delete",
    };
    schemas
        .get(&format!("{method}:{}", normalized(endpoint)))
        .cloned()
}

pub(crate) fn validate(schema: &Value, value: &Value) -> Result<()> {
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(schema)?;
    // Do not include instance values in errors; they can contain credentials.
    let errors: Vec<_> = validator
        .iter_errors(value)
        .take(12)
        .map(|e| format!("{} (schema {})", e.instance_path(), e.schema_path()))
        .collect();
    if !errors.is_empty() {
        bail!(
            "Request does not match the advertised tool schema: {}. Read tools/list for required fields and types; preserve identifiers from inventory.",
            errors.join("; ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolKind, tool_model, tools};

    #[test]
    fn every_advertised_integration_write_schema_compiles() {
        for tool in tools::iter() {
            if let ToolKind::IntegrationWrite {
                method,
                endpoint,
                body_required,
                ..
            } = tool.kind
            {
                if body_required {
                    assert!(body(method, endpoint).is_some(), "{}", tool.name);
                }
                let schema = Value::Object(tool_model(tool).input_schema.as_ref().clone());
                jsonschema::options()
                    .should_validate_formats(true)
                    .build(&schema)
                    .unwrap();
            }
        }
    }

    #[test]
    fn firewall_contract_accepts_scoped_policy_and_rejects_guesses() {
        let schema = body(IntegrationMethod::Post, "firewall/policies").unwrap();
        let mut policy = json!({
            "name":"Block test", "enabled":true, "loggingEnabled":false,
            "action":{"type":"BLOCK"}, "ipProtocolScope":{"ipVersion":"IPV4_AND_IPV6"},
            "source":{"zoneId":"11111111-1111-4111-8111-111111111111", "trafficFilter":{"type":"NETWORK","networkFilter":{"matchOpposite":false,"networkIds":["22222222-2222-4222-8222-222222222222"]}}},
            "destination":{"zoneId":"33333333-3333-4333-8333-333333333333"}
        });
        validate(&schema, &policy).unwrap();
        policy["source"]["matchingTarget"] = json!("NETWORK");
        assert!(validate(&schema, &policy).is_err());
        policy["source"]
            .as_object_mut()
            .unwrap()
            .remove("matchingTarget");
        policy["source"]["zoneId"] = json!("corrupted-identifier");
        assert!(validate(&schema, &policy).is_err());
    }

    #[test]
    fn ordering_patch_and_manifest_override_are_distinct() {
        validate(
            &body(IntegrationMethod::Put, "firewall/policies/ordering").unwrap(),
            &json!({"orderedFirewallPolicyIds":{"beforeSystemDefined":[],"afterSystemDefined":[]}}),
        )
        .unwrap();
        validate(
            &body(IntegrationMethod::Patch, "firewall/policies/{id}").unwrap(),
            &json!({"loggingEnabled":true}),
        )
        .unwrap();
        assert!(
            validate(
                &body(IntegrationMethod::Patch, "firewall/policies/{id}").unwrap(),
                &json!({"loggingEnabled":"yes"})
            )
            .is_err()
        );
        let tool = crate::UnifiMcp::find_tool("unifi_create_firewall_policy").unwrap();
        let schema = Value::Object(tool_model(tool).input_schema.as_ref().clone());
        assert!(schema["properties"]["body"]["properties"]["action"].is_object());
        assert!(validate(&schema, &json!({"policy_data":{},"confirm":false})).is_err());
    }
}
