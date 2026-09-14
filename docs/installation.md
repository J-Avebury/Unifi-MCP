# Installation and client setup

This repository currently documents source-based installation only. It is not
published to crates.io, Homebrew, an MCP marketplace, or GitHub Releases yet.
Build the binary locally and keep credentials outside the repository.

## Choose a transport

| Use case | Transport | Server location | Recommended guide |
| --- | --- | --- | --- |
| One AI client on one computer | stdio | Local machine | [Local build](#local-build) |
| Several clients on a private network | Streamable HTTP | A secured host | [Remote HTTP](#remote-http) |
| ChatGPT custom app | Streamable HTTP | Public HTTPS or approved secure tunnel | [ChatGPT](clients/chatgpt.md) |

Stdio is the default and is the safest starting point. Streamable HTTP is
already implemented, but the server should be placed behind TLS or a private
tunnel. Do not expose the unauthenticated development bind address to the
Internet.

## Local build

Requirements: Rust stable and a working network connection for Cargo dependencies.

```sh
cargo build --release
```

The binary is:

```text
target/release/unifi-mcp
```

Use either direct controller credentials or the Site Manager connector. Prefer
secret files:

```sh
chmod 600 /secure/path/unifi-api-key
```

For direct controller access:

```text
UNIFI_NETWORK_BASE_URL=https://controller.example
UNIFI_NETWORK_SITE=default
UNIFI_NETWORK_API_KEY_FILE=/secure/path/unifi-api-key
```

For cloud-proxied access:

```text
UNIFI_SITE_MANAGER_API_KEY_FILE=/secure/path/unifi-site-manager-api-key
UNIFI_SITE_MANAGER_CONSOLE_ID=<console-id>
UNIFI_NETWORK_SITE=default
```

When both are configured, cloud reads are preferred and direct reads are the
fallback. Mutations are not automatically retried through a second route.

## Remote HTTP

Start the server on a private bind address with a separate MCP bearer token:

```sh
UNIFI_SITE_MANAGER_API_KEY_FILE=/secure/path/unifi-site-manager-api-key \\
UNIFI_SITE_MANAGER_CONSOLE_ID=<console-id> \\
UNIFI_NETWORK_SITE=default \\
UNIFI_MCP_TRANSPORT=http \\
UNIFI_MCP_HTTP_TOKEN_FILE=/secure/path/unifi-mcp-http-token \\
UNIFI_MCP_HTTP_BIND=127.0.0.1:8000 \\
UNIFI_MCP_HTTP_ALLOWED_HOST=mcp.example.com \\
target/release/unifi-mcp
```

Terminate TLS at a reverse proxy or use a private tunnel. The MCP endpoint is
normally `/mcp`; configure the proxy to preserve the HTTP streaming response.
Use the client’s remote HTTP configuration rather than placing the UniFi API
key in the client’s MCP header. The server itself reads the UniFi credential.

## First check

For stdio clients, ask the client to list the tools and call
`unifi_get_network_health`. For a write-capable client, call a harmless
mutation without `confirm: true` and verify that it returns a preview instead
of changing the controller.

## Safety rules

- Keep API keys and bearer tokens in secret files or the client’s secret store.
- Do not commit `.env`, `.secrets`, controller URLs containing credentials, or
  copied controller responses.
- Start with read-only tools. Every supported mutation requires an explicit
  `confirm: true` call.
- `unifi_batch` is read-only; do not use it to attempt a write.
- A controller capability error is meaningful. Do not replace it with a
  different site or silently substitute a route.

## Client guides

- [Claude Desktop](clients/claude-desktop.md)
- [Claude Code](clients/claude-code.md)
- [Codex](clients/codex.md)
- [Cursor](clients/cursor.md)
- [VS Code](clients/vscode.md)
- [ChatGPT](clients/chatgpt.md)
- [Generic MCP clients](clients/generic.md)
- [Optional UniFi operator skill](skills/unifi-network-operator/SKILL.md). Native copies are provided in `.claude/skills/`, `.agents/skills/`, and `.github/skills/`; Cursor uses `.cursor/rules/`.
