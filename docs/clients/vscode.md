# VS Code

VS Code uses an `mcp.json` file with a `servers` object. Put a project-scoped
configuration in `.vscode/mcp.json`, or use the user MCP configuration from the
Command Palette.

## Local stdio

```json
{
  "servers": {
    "unifi": {
      "type": "stdio",
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

## Remote HTTP with an input prompt

```json
{
  "inputs": [
    {
      "type": "promptString",
      "id": "unifi-mcp-token",
      "description": "UniFi MCP bearer token",
      "password": true
    }
  ],
  "servers": {
    "unifi-remote": {
      "type": "http",
      "url": "https://mcp.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${input:unifi-mcp-token}"
      }
    }
  }
}
```

Use **MCP: List Servers** to inspect, restart, or disable the server. If
sandboxing is enabled, allow the domains the server needs, including your MCP
host and `api.ui.com` when the local process itself makes cloud connector
requests. Reference: [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration).
