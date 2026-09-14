# Legacy controller writes

Some UniFi consoles expose the newer Network Integration write routes in the
catalogue but reject them at runtime. This MCP keeps those official tools and
adds explicit legacy tools for controllers where the older routes are the
actual supported contract.

## Available tools

- `unifi_update_legacy_network` writes `PUT rest/networkconf/{network_id}`.
  The MCP fetches the existing network, merges the requested `update_data`,
  writes the complete object, and reads it back to verify the result.
- `unifi_create_legacy_firewall_rule` writes `POST rest/firewallrule`.
  The request body must use the legacy firewall-rule schema supported by the
  controller. Use `unifi_list_legacy_firewall_rules` first to inspect the
  fields and conventions already present on that console.

Both tools require an explicit confirmation after a preview. Mutations use one
selected route and are not silently retried through another route after an
ambiguous failure.

## Important schema distinction

The official `unifi_create_firewall_policy` tool uses the newer Integration
policy model, including zone identifiers and network IDs. A legacy firewall
rule is a different controller object. The MCP therefore does not translate a
body such as `source.zone_id`, `destination.zone_id`, and `network_ids` into a
legacy rule by guesswork. Supply a controller-compatible legacy rule body
instead.

Likewise, `unifi_update_network` remains the official Integration API tool.
It requires the full camelCase Network create/update schema; legacy fields such
as `update_data`, `network_isolation_enabled`, and `upnp_lan_enabled` are
rejected before a connector request is made. If the desired change uses those
legacy fields, use the explicit legacy network tool rather than relying on an
automatic cross-route write fallback. This preserves the requested route
contract and avoids applying a subtly wrong configuration model.

The official Integration firewall-policy tool likewise requires the v10.4.57
shape: an action object such as `{ "type": "BLOCK" }`, `zoneId`,
`ipProtocolScope`, and `loggingEnabled`. The MCP rejects legacy fields such as
`matching_target`, `matching_target_type`, and `zone_id` locally instead of
sending a request that the controller will reject.

Always preview first, inspect the generated route and redacted body, then
confirm only after checking the target site and identifiers.
