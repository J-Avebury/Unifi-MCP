---
name: unifi-network-operator
description: Safe operating guidance for the UniFi Network MCP. Use when inspecting a UniFi controller, diagnosing network state, or changing Network configuration through this MCP.
---

# UniFi Network operator guidance

Use this skill alongside the configured `unifi-mcp` server. It is guidance for
an AI client, not a replacement for the MCP connection.

## Operating rules

1. Establish the intended site before calling tools. Do not substitute another
   site when a site ID cannot be resolved exactly.
2. Start with read-only inventory or health tools. Prefer the narrowest tool
   that answers the question.
3. Treat controller capability errors as real. Report the unsupported route
   instead of inventing an empty result or trying a different site.
4. Before any mutation, inspect the target and call the mutation without
   `confirm: true` to obtain its preview. Repeat only after the operator gives
   explicit approval and the target, requested fields, and consequence match.
5. Never use `unifi_batch` for mutations; it is bounded and read-only.
6. Do not reveal API keys, bearer tokens, passwords, VPN material, SNMP
   communities, SSH credentials, or raw sensitive controller payloads.
7. After a confirmed write, report the controller response and verification
   result. Do not automatically retry a write through a second transport after
   a timeout or ambiguous response.

## Useful first calls

- `unifi_tool_index` to find the exact tool.
- `unifi_get_network_health` for a compact health check.
- `unifi_list_connected_clients` and `unifi_list_adopted_devices` for inventory.
- `unifi_batch` for a small set of read-only calls.

## Remote deployments

When the server is configured with Site Manager credentials, cloud reads are
preferred and direct controller reads are the fallback. The MCP bearer token
and UniFi API key are separate credentials. Keep both outside prompts,
repository files, and tool arguments.
