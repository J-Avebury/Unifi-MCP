---
name: unifi-network-operator
description: Safe operating guidance for the UniFi Network MCP. Use when inspecting a UniFi controller, diagnosing network state, discovering Site Manager consoles, or changing Network configuration through this MCP.
---

# UniFi Network operator guidance

Use this skill alongside the configured `unifi-mcp` server. It is guidance for
an AI client, not a replacement for the MCP connection.

## Operating rules

1. Establish the intended site before calling tools. Do not substitute another
   site when a site ID cannot be resolved exactly.
2. If Site Manager is configured and the console is unknown, call
   `unifi_list_site_manager_consoles` first. Use `page_size`, `next_token`, and
   `query` to narrow large inventories. Never guess a console ID or silently
   choose one from multiple results; selection is made with
   `UNIFI_SITE_MANAGER_CONSOLE_ID`.
3. Start with read-only inventory or health tools. Prefer the narrowest tool
   that answers the question.
4. Use the tool catalogue and its current input schema, not remembered UniFi
   payloads. Integration bodies use the official camelCase schema. Do not
   invent snake_case aliases, wrappers, or fields from a legacy controller API.
5. Before any mutation, inspect the target and call the mutation without
   `confirm: true` to obtain its preview. Repeat only after the operator gives
   explicit approval and the target, requested fields, and consequence match.
6. Never use `unifi_batch` for mutations; it is bounded and read-only.
7. For Network Integration writes, send the complete official body where the
   endpoint is a full create/update contract. For firewall policies use an
   action object such as `{ "type": "BLOCK" }`, `zoneId`,
   `ipProtocolScope`, and `loggingEnabled`.
8. Legacy controller fields such as `update_data`,
   `network_isolation_enabled`, `upnp_lan_enabled`, `matching_target`, and
   `zone_id` belong on explicit legacy tools. Use `unifi_update_legacy_network`
   or `unifi_create_legacy_firewall_rule` rather than translating them into an
   Integration body.
9. Do not reveal API keys, bearer tokens, passwords, VPN material, SNMP
   communities, SSH credentials, or raw sensitive controller payloads.
10. After a confirmed write, report the controller response and verification
    result. Do not automatically retry a write through a second transport after
    a timeout or ambiguous response. A 400 response is a schema or controller
    capability signal; inspect it rather than repeating the same payload.

## Useful first calls

- `unifi_tool_index` to find the exact tool and current schema.
- `unifi_list_site_manager_consoles` to discover permitted cloud consoles.
- `unifi_get_network_health` for a compact health check.
- `unifi_list_connected_clients` and `unifi_list_adopted_devices` for inventory.
- `unifi_batch` for a small set of read-only calls.

## Remote deployments

When the server is configured with Site Manager credentials, cloud reads are
preferred and direct controller reads are the fallback. The MCP bearer token
and UniFi API key are separate credentials. Keep both outside prompts,
repository files, and tool arguments.
