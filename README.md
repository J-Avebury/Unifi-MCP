# Unifi-MCP

Local, on-demand MCP server for UniFi Network and UniFi Talk.

This is a Rust stdio MCP. It does not listen on a TCP port. An MCP client starts
the binary as a child process and talks to it over stdin/stdout.

## Tools

- `unifi_tool_index` - compact list of tools exposed by this server.
- `unifi_network_status` - compact summary of devices, online clients, Wi-Fi
  networks, and rogue APs.
- `unifi_list_devices` - read-only UniFi Network device inventory.
- `unifi_list_clients` - read-only client inventory, with optional online-only and
  query filters.
- `unifi_list_wlans` - configured Wi-Fi networks.
- `unifi_list_rogue_aps` - rogue access points detected by UniFi.
- `unifi_list_talk_sites` - UniFi Talk sites visible to the API key.
- `unifi_raw_network_endpoint` - constrained read-only escape hatch for allowlisted
  UniFi Network endpoints.

The current Network endpoint allowlist is:

- `stat/sta`
- `stat/alluser`
- `stat/rogueap`
- `list/wlanconf`
- `stat/device`

## Configuration

The server reads configuration from environment variables:

Server-specific variables take priority over shared `UNIFI_*` fallbacks.

| Server-specific | Shared fallback | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `UNIFI_NETWORK_BASE_URL` | `UNIFI_BASE_URL` | No | `https://your-unifi-console.example` | Full UniFi controller URL. |
| `UNIFI_NETWORK_HOST` | `UNIFI_HOST` | No | | Controller host if `*_BASE_URL` is not set. |
| `UNIFI_NETWORK_PORT` | `UNIFI_PORT` | No | `443` | Used only with `*_HOST`. |
| `UNIFI_NETWORK_SITE` | `UNIFI_SITE` | No | `default` | UniFi Network site id. |
| `UNIFI_NETWORK_API_KEY` | `UNIFI_API_KEY` | Yes | | UniFi API key. Do not commit this. |
| `UNIFI_NETWORK_API_KEY_FILE` | `UNIFI_API_KEY_FILE` | No | | File containing the API key. |
| `UNIFI_NETWORK_VERIFY_SSL` | `UNIFI_VERIFY_SSL` | No | | Set to `false` for a local self-signed controller certificate. |
| `UNIFI_NETWORK_INSECURE_TLS` | `UNIFI_INSECURE_TLS` | No | `false` | Alternative TLS flag when `*_VERIFY_SSL` is not set. |
| `UNIFI_NETWORK_REDACT_SENSITIVE_FIELDS` | `UNIFI_REDACT_SENSITIVE_FIELDS` | No | `true` | Redacts known secret fields before MCP responses. |
| `RUST_LOG` | | No | | Rust tracing filter, for example `info`. |

The server does not load `.env` files automatically. Put env values in the MCP
client config, export them in the launcher, or point `*_API_KEY_FILE` at a
trusted file.

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
