# UniFi MCP agent guidance

Use the MCP server according to [the operator skill](.github/skills/unifi-network-operator/SKILL.md).

- Discover Site Manager consoles with `unifi_list_site_manager_consoles` when the target console is not already selected; never guess among multiple consoles.
- Prefer read-only health and inventory tools first.
- Confirm the intended site exactly; never silently substitute another site.
- Mutations must be previewed and require explicit `confirm: true`.
- `unifi_batch` is read-only.
- Keep UniFi API keys, bearer tokens, passwords, and raw sensitive controller output out of commits and responses.
- Treat unsupported-route errors as genuine controller capability limits.
- Use official camelCase Integration schemas; use explicit legacy tools for legacy fields such as `update_data` or `zone_id`.
