# Claude Code

Build the binary first, then register it with Claude Code’s MCP command.

## Local stdio

```sh
claude mcp add --scope user \\
  -e UNIFI_SITE_MANAGER_API_KEY_FILE=/secure/path/unifi-site-manager-api-key \\
  -e UNIFI_SITE_MANAGER_CONSOLE_ID=<console-id> \\
  -e UNIFI_NETWORK_SITE=default \\
  -e UNIFI_NETWORK_REDACT_SENSITIVE_FIELDS=true \\
  unifi -- /absolute/path/to/Unifi-MCP/target/release/unifi-mcp
```

Check the registration:

```sh
claude mcp list
claude mcp get unifi
```

For a project-scoped configuration, use `--scope project`; review the
project’s generated `.mcp.json` before committing it, particularly if it
contains environment values.

## Remote HTTP

```sh
claude mcp add --scope user --transport http \\
  --header "Authorization: Bearer <mcp-bearer-token>" \\
  unifi-remote https://mcp.example.com/mcp
```

Avoid putting a real bearer token in shell history. Prefer Claude’s connector
settings or a credential mechanism provided by the deployment when available.

Reference: [Anthropic’s Claude Code MCP guide](https://docs.anthropic.com/en/docs/claude-code/mcp).
