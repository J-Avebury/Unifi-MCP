# Unifi-MCP

Local, on-demand Rust MCP server for UniFi Network.

This is a Rust stdio MCP. It does not listen on a TCP port. An MCP client starts
the binary as a child process and talks to it over stdin/stdout.

## Compatibility target

The tool names and response contract are being brought into parity with the
Network server from [`the upstream compatibility project`](),
implemented natively in Rust. The current release exposes 73 read-only tools
covering discovery, batching, dashboards, devices, clients, WLANs, networks,
events, alarms, routing, firewall inventory, switching, statistics, DPI, and
system settings.

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
| `RUST_LOG` | | No | | Rust tracing filter, for example `info`. |

The server requires a controller URL/host and accepts either an API key or a
local username/password pair. An API key takes precedence when both are
configured. The server does not load `.env`
files automatically. Put env values in the MCP client config, export them in the
launcher, or point a supported secret variable at a trusted `*_FILE`.

Known secret-bearing fields are redacted by default, including Wi-Fi
passphrases, passwords, API keys, tokens, VPN key material, SNMP community
strings, and device SSH credentials.

## Build

```sh
cargo build --release
```

## MCP client config

Use `mcp-config.example.json` as the starting point. The important part is that
the MCP client should run the compiled binary directly and pass the UniFi values
as environment variables.
