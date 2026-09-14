# Generic MCP clients

For a client that supports MCP directly, choose one of these configurations.

## Stdio

```json
{
  "mcpServers": {
    "unifi": {
      "command": "/absolute/path/to/Unifi-MCP/target/release/unifi-mcp",
      "env": {
        "UNIFI_SITE_MANAGER_API_KEY_FILE": "/secure/path/unifi-site-manager-api-key",
        "UNIFI_SITE_MANAGER_CONSOLE_ID": "<console-id>",
        "UNIFI_NETWORK_SITE": "default"
      }
    }
  }
}
```

## Streamable HTTP

```json
{
  "mcpServers": {
    "unifi-remote": {
      "type": "http",
      "url": "https://mcp.example.com/mcp",
      "headers": {
        "Authorization": "Bearer <mcp-bearer-token>"
      }
    }
  }
}
```

Some clients call the local key `mcpServers`; VS Code uses `servers` and a
`type` field. Follow the client-specific guide where one exists.
