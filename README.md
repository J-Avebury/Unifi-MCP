# Unifi-MCP

Local, on-demand Rust MCP server for UniFi Network.

This is a Rust MCP with stdio by default and authenticated Streamable HTTP for remote clients. Stdio runs the binary as a child process; HTTP mode lets a client connect from anywhere through a secured deployment.

## Compatibility target

The tool names and response contract target the official local UniFi Network
API, implemented natively in Rust. The current release exposes 249 Network tools: the official Integration API surface, legacy diagnostics, and all 194 exact upstream Network compatibility names. Compatibility routes remain controller-dependent and report explicit unsupported-route errors. Against the configured 10.6.101 controller, the latest non-mutating read smoke test covered 59 read tools: 58 succeeded and one returned an explicit capability error: batch status is not exposed by this controller and this MCP executes batches synchronously.

The checked-in upstream Network manifest supplies exact compatibility schemas and annotations for the 194-name surface. Identified compatibility PUT updates fetch, merge, write, and verify the requested fields; command-style routes disclose when stable read-back is unavailable.

The Integration API inventory tools resolve the controller's UUID site ID from
the configured legacy site reference before making a request. They require an
API key and deliberately refuse to fall back to a different site if no exact
mapping exists. Controllers that do not support a given Integration endpoint
return a clear capability error rather than an ambiguous empty result.

Every tool returns the standard envelope used by the reference server:

```json
{"success": true, "data": {}}
```

Handled failures return `{"success": false, "error": "..."}`. Full results are
also supplied as MCP structured content.

The reference project currently supports Network, Protect, and Access. It does
not expose a supported UniFi Talk server, so this project no longer advertises
the former non-working Talk endpoint.

Use `unifi_tool_index` to browse the catalogue, `unifi_execute` for indirect
execution, and `unifi_batch` for bounded read-only batches. MCP clients may also
discover and invoke every domain tool directly.

The initial mutation batch also includes client block/unblock/reconnect and
device reboot/upgrade actions. Each returns a preview by default and performs
no controller write until the caller repeats it with `confirm: true`. Mutations
are refused by `unifi_batch`; they must be targeted and confirmed individually.

The official Network API mutation tools use the same preview/confirmation
boundary and accept a `body` object matching the version-specific API schema.
Live mutation validation is deferred until the Network implementation phase is
complete.

## Raw endpoint allowlist

The current Network endpoint allowlist is:

- `stat/sta`
- `stat/alluser`
- `stat/rogueap`
- `list/wlanconf`
- `stat/device`
- `stat/health`
- `stat/event`
- `stat/alarm`
- `rest/networkconf`
- `rest/portforward`
- `rest/routing`
- `rest/firewallgroup`
- `rest/firewallrule`
- `rest/portconf`
- `rest/dynamicdns`
- `list/usergroup`
- `stat/voucher`
- `get/setting`
- `stat/sysinfo`
- `stat/sitedpi`

## Configuration

The server reads configuration from environment variables:

Server-specific variables take priority over shared `UNIFI_*` fallbacks.

| Server-specific | Shared fallback | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `UNIFI_NETWORK_BASE_URL` | `UNIFI_BASE_URL` | Yes* | | Full UniFi controller URL; configure locally and do not commit it. |
| `UNIFI_NETWORK_HOST` | `UNIFI_HOST` | No | | Controller host if `*_BASE_URL` is not set. |
| `UNIFI_NETWORK_PORT` | `UNIFI_PORT` | No | `443` | Used only with `*_HOST`. |
| `UNIFI_NETWORK_SITE` | `UNIFI_SITE` | No | `default` | UniFi Network site id. |
| `UNIFI_NETWORK_API_KEY` | `UNIFI_API_KEY` | Yes | | UniFi API key. Do not commit this. |
| `UNIFI_NETWORK_API_KEY_FILE` | `UNIFI_API_KEY_FILE` | No | | File containing the API key. |
| `UNIFI_NETWORK_USERNAME` | `UNIFI_USERNAME` | Conditional | | Local controller account. Required with a password when no API key is supplied. |
| `UNIFI_NETWORK_PASSWORD` | `UNIFI_PASSWORD` | Conditional | | Local controller password. |
| `UNIFI_NETWORK_PASSWORD_FILE` | `UNIFI_PASSWORD_FILE` | No | | File containing the local controller password. |
| `UNIFI_NETWORK_VERIFY_SSL` | `UNIFI_VERIFY_SSL` | No | | Set to `false` for a local self-signed controller certificate. |
| `UNIFI_NETWORK_INSECURE_TLS` | `UNIFI_INSECURE_TLS` | No | `false` | Alternative TLS flag when `*_VERIFY_SSL` is not set. |
| `UNIFI_NETWORK_REDACT_SENSITIVE_FIELDS` | `UNIFI_REDACT_SENSITIVE_FIELDS` | No | `true` | Redacts known secret fields before MCP responses. |
| `UNIFI_SITE_MANAGER_API_KEY_FILE` | `UNIFI_CLOUD_API_KEY_FILE` | Conditional | | Site Manager API key file for the api.ui.com console connector. |
| `UNIFI_SITE_MANAGER_CONSOLE_ID` | `UNIFI_CLOUD_CONSOLE_ID` | Conditional | | Console ID used by the api.ui.com connector. |
| `UNIFI_SITE_MANAGER_SITE_ID` | `UNIFI_CLOUD_SITE_ID` | No | | Optional Site Manager site ID for cloud dashboard metrics. |
| `UNIFI_MCP_TRANSPORT` | `UNIFI_NETWORK_MCP_TRANSPORT` | No | `stdio` | Set to `http` for authenticated Streamable HTTP. |
| `UNIFI_MCP_HTTP_TOKEN_FILE` | `UNIFI_NETWORK_MCP_HTTP_TOKEN_FILE` | Conditional for HTTP | | Bearer token file required by HTTP mode. |
| `UNIFI_MCP_HTTP_BIND` | `UNIFI_NETWORK_MCP_HTTP_BIND` | No | `127.0.0.1:8000` | HTTP listen address. |
| `UNIFI_MCP_HTTP_ALLOWED_HOST` | `UNIFI_NETWORK_MCP_HTTP_ALLOWED_HOST` | No | bind address | Host header accepted by the Streamable HTTP server. |
| `RUST_LOG` | | No | | Rust tracing filter, for example `info`. |

The server accepts local controller credentials, Site Manager cloud credentials, or both. Cloud connector mode requires a console ID and can operate without a local controller URL. When both transports are configured, Site Manager is preferred for reads and the direct controller route is used as a read fallback; writes are never automatically retried through a second route. HTTP mode requires a bearer token and should normally be placed behind TLS or a private network. An API key takes precedence when both are
configured. The server does not load `.env`
files automatically. Put env values in the MCP client config, export them in the
launcher, or point a supported secret variable at a trusted `*_FILE`.

Known secret-bearing fields are redacted by default, including Wi-Fi
passphrases, passwords, API keys, tokens, VPN key material, SNMP community
strings, and device SSH credentials.

## Remote deployment

For remote MCP clients, run authenticated Streamable HTTP behind a TLS reverse
proxy or private tunnel. The Site Manager connector forwards Network requests
through `api.ui.com`; the connector supports the upstream HTTP methods used by
the tools, including GET, POST, PUT, PATCH, and DELETE. The API key and bearer
token are read from files in this example and remain outside the repository:

```sh
UNIFI_SITE_MANAGER_API_KEY_FILE=/run/secrets/unifi-site-manager-api-key \
UNIFI_SITE_MANAGER_CONSOLE_ID=<console-id> \
UNIFI_NETWORK_SITE=default \
UNIFI_MCP_TRANSPORT=http \
UNIFI_MCP_HTTP_TOKEN_FILE=/run/secrets/unifi-mcp-http-token \
UNIFI_MCP_HTTP_BIND=127.0.0.1:8000 \
UNIFI_MCP_HTTP_ALLOWED_HOST=mcp.example.com \
./target/release/unifi-mcp
```

The connector path for a site-scoped Integration request is, for example,
`/v1/connector/consoles/{consoleId}/proxy/network/integration/v1/sites/{siteId}/dns/policies`.
The console ID must be one the Site Manager key is permitted to access. The
connector itself enforces its request timeout and response-size limits; this
client also bounds its connector responses and reports unsupported routes
explicitly.

## Build

```sh
cargo build --release
```

## MCP client config

Use `mcp-config.example.json` as the starting point. The important part is that
the MCP client should run the compiled binary directly and pass the UniFi values
as environment variables.
