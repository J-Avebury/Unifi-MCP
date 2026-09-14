# Cursor

Cursor supports local stdio and remote MCP servers. Configure a project server
in `.cursor/mcp.json`, or put the same definition in `~/.cursor/mcp.json` for
all projects.

## Local stdio

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

## Remote HTTP

```json
{
  "mcpServers": {
    "unifi-remote": {
      "url": "https://mcp.example.com/mcp",
      "headers": {
        "Authorization": "Bearer <mcp-bearer-token>"
      }
    }
  }
}
```

Keep project configuration free of real tokens. Cursor can also manage MCP
servers from its UI and CLI. Reference: [Cursor MCP documentation](https://docs.cursor.com/context/model-context-protocol).
