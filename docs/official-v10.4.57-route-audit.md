# UniFi Network v10.4.57 route audit

This audit checks the Rust MCP against the official Network v10.4.57 contract
and the generated connector examples, including the Go examples supplied by
UniFi. The official OpenAPI document contains 44 path families and 73 HTTP
operations.

## Connector contract

For a cloud request, the MCP constructs:

```text
https://api.ui.com/v1/connector/consoles/{consoleId}/proxy/{path}
```

For a site-scoped Network Integration operation, `{path}` is:

```text
network/integration/v1/sites/{siteId}/{endpoint}
```

For a global Integration operation, `{path}` is:

```text
network/integration/{endpoint}
```

The MCP sends `Accept: application/json`, `X-API-Key`, and JSON request bodies
where applicable. `reqwest` supplies the JSON content type for body-bearing
requests. The API key is never included in logs. Cloud connector requests use
the documented 25-second timeout and 10 MB response limit.

## Coverage

All v10.4.57 operation families are represented:

- application information, countries, DPI applications/categories, pending
  devices, and local sites;
- clients and client actions;
- devices, adoption/removal, device actions, port actions, statistics, and
  device tags;
- networks, network references, Wi-Fi broadcasts, and DNS policies;
- firewall zones, firewall policies, ACL rules, and ordering operations;
- hotspot vouchers and voucher actions;
- WAN interfaces, RADIUS profiles, VPN servers, and site-to-site tunnels;
- switch stacks, MC-LAG domains, and link aggregation groups;
- traffic matching lists.

The MCP now includes a dedicated `unifi_get_api_acl_rule_ordering` read tool.
Firewall-policy and ACL ordering reads use a no-argument site-scoped GET;
they are not modelled as query-object tools. The existing
`unifi_get_firewall_policy_ordering` and `unifi_get_api_firewall_policy_ordering`
names are aliases for the same official route.

## Console discovery and selection

`unifi_list_site_manager_consoles` queries the Site Manager host inventory with
bounded `page_size`, `next_token`, and current-page `query` filtering. It
returns only console summaries and the pagination token needed to continue.
The MCP never silently chooses one console from a large inventory: set
`UNIFI_SITE_MANAGER_CONSOLE_ID` to the selected `id`. A one-console setup can
use that same explicit configuration without any special-case guessing.

## Routing and safety notes

The site-scoped official tools use Integration-first routing. If a controller
or cloud connector reports an unsupported read route, the MCP may try the
versioned v2 equivalent where one is documented by the existing controller
compatibility surface. Writes use the selected Integration route and are not
silently retried through a different write contract.

The generic `body` field remains intentionally open at the MCP boundary so the
controller-version-specific schema can be passed through without lossy Rust
reconstruction. The route, HTTP method, required identifiers, confirmation
boundary, and post-write behaviour are implemented by the MCP. A body that is
valid according to the official schema can therefore be previewed without
being rewritten.

This audit establishes route and transport parity; it does not claim that
every v10.4.57 operation is enabled on every console. The configured
controller's firmware and permissions remain authoritative. In particular,
a controller returning HTTP 400 for an official write is a controller
capability or validation result, not evidence that the connector route was
misconstructed.
