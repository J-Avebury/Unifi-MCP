# UniFi MCP agent guidance

Use the MCP server according to [the operator skill](.github/skills/unifi-network-operator/SKILL.md).

- Prefer read-only health and inventory tools first.
- Confirm the intended site exactly; never silently substitute another site.
- Mutations must be previewed and require explicit `confirm: true`.
- `unifi_batch` is read-only.
- Keep UniFi API keys, bearer tokens, passwords, and raw sensitive controller output out of commits and responses.
- Treat unsupported-route errors as genuine controller capability limits.
