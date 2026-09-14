# MCP request contract

Integration write tools advertise their actual request bodies through MCP
`tools/list`. These schemas come from the pinned official
[Network v10.4.57 OpenAPI document](https://developer.ui.com/network/v10.4.57/openapi.json),
stored in `src/integration_openapi.json`. The `integration_schema` module
resolves references, flattens inheritance and represents discriminator variants
as `oneOf`. Unknown object fields are rejected deliberately to catch legacy
payloads and misspellings. The same schemas validate calls before preview or
HTTP requests, including calls through `unifi_execute`.

The upstream compatibility manifest no longer overrides Integration write
schemas or descriptions. These tools accept `body`, not `policy_data`.
Method-specific schemas distinguish full network/policy PUTs, policy ordering,
and the partial firewall PATCH. UUID validation catches malformed identifiers;
it cannot prove that a syntactically valid UUID identifies the intended object.
Use inventory to establish that identity and preserve the exact values.

Legacy network updates accept flat `body`, top-level `update_data`, or the
previously used sole `body.update_data` wrapper. The wrapper is removed before
preview, merge and verification. Mixed wrapper siblings are rejected. Existing
fields are retained during merge. A legacy GET must yield exactly one record.

A preview establishes local structural validity only. It does not establish
controller support, permissions, target identity or firewall equivalence.
Integration write responses explicitly state when read-back verification was
not performed. Read the resulting target before claiming applied state. A
failed policy payload does not authorise switching to a legacy LAN_IN rule.

Essential instructions are also sent in the MCP initialisation response so
clients receive them even when repository skills are not installed. Restart
the MCP process and refresh its tool catalogue after updating the binary.
