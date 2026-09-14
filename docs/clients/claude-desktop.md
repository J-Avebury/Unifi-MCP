# Claude Desktop

Claude Desktop supports local MCP servers through its desktop configuration or
through desktop extensions. This project currently supplies a local binary and
configuration example, not a `.dxt` extension.

## Local stdio setup

1. Build the binary as described in [installation](../installation.md).
2. Add the server to Claude Desktop’s `claude_desktop_config.json`.
3. Restart Claude Desktop and verify that the UniFi tools appear.

Use this shape, replacing placeholders with real local paths and values:

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

Do not put the actual API key in this JSON. A file path is not a secret, but
the file must be readable only by the account running Claude Desktop.

## Remote setup

For a remote deployment, add the HTTPS MCP endpoint through Claude’s
**Settings → Connectors** rather than putting a remote server in
`claude_desktop_config.json`. Enter the HTTPS endpoint and authenticate using
the mechanism provided by the deployment. This project’s current HTTP mode
uses a bearer token, not OAuth; use a reverse proxy or tunnel if your client
requires OAuth.

Claude’s remote connector support is separate from its local stdio support.
See Anthropic’s [local MCP guide](https://support.anthropic.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop)
and [remote MCP connector guide](https://support.anthropic.com/en/articles/11503834-building-custom-integrations-via-remote-mcp).
