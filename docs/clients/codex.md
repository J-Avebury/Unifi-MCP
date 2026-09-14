# Codex

Codex supports both local stdio and remote Streamable HTTP MCP servers.

## Local stdio

```sh
codex mcp add unifi \\
  --env UNIFI_SITE_MANAGER_API_KEY_FILE=/secure/path/unifi-site-manager-api-key \\
  --env UNIFI_SITE_MANAGER_CONSOLE_ID=<console-id> \\
  --env UNIFI_NETWORK_SITE=default \\
  -- /absolute/path/to/Unifi-MCP/target/release/unifi-mcp
```

Verify it:

```sh
codex mcp list
codex mcp get unifi
```

## Remote HTTP

```sh
codex mcp add unifi-remote \\
  --url https://mcp.example.com/mcp \\
  --bearer-token-env-var UNIFI_MCP_HTTP_TOKEN
```

Export `UNIFI_MCP_HTTP_TOKEN` only in the local shell or secret manager used
to launch Codex. Do not put the token in a checked-in config file.
