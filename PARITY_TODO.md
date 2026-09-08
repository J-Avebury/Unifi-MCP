# Rust parity tracker

This is the visible implementation tracker for parity with the current
[`the upstream compatibility project`]() reference. The
comparison baseline is upstream commit `redacted`.

## Current position

**Active priority: Network only.** Do not begin Protect or Access implementation
until Network reaches its 194-tool parity target and the Network definition of
done below passes.

| Surface | Upstream tools | Rust tools | Exact-name parity | Status |
| --- | ---: | ---: | ---: | --- |
| Network | 194 | 78 | 77/194 (39.7%) | In progress |
| Protect | 62 | 0 | 0/62 | Not started |
| Access | 37 | 0 | 0/37 | Not started |
| Total | 293 | 78 | 77/293 (26.3%) | Not at parity |

`unifi_raw_network_endpoint` is intentionally Rust-specific, hence the one
additional catalogue entry that does not count toward exact upstream-name
parity.

## Completed

- [x] Read-only Network foundation: local/API-key auth, cookie handling,
  transient retry, sensitive-field redaction, structured MCP responses, and
  total/returned result counts.
- [x] Network meta tools: `unifi_tool_index`, `unifi_execute`, and a bounded,
  read-only `unifi_batch`.
- [x] 46 read/diagnostic tools across dashboard, devices, clients, WLANs,
  networks, routing, firewall inventory, events, alarms, statistics, and
  settings.
- [x] Initial mutation contract: explicit MCP annotations, preview before
  `confirm: true`, and batch-level write rejection.
- [x] Initial mutations: block/unblock/reconnect client; reboot/upgrade device.
- [x] Integration-v2 API boundary: API-key requirement, verified local site-ID
  discovery, cached UUID mapping, and capability-aware 404 responses.
- [x] V2 inventory/detail tools: ACL rules, AP groups, client groups, content
  filters, DNS, firewall policies/zones, QoS rules, and traffic routes.
- [x] Legacy inventory/detail tools: Dynamic DNS, hotspot vouchers, and VPN
  clients/servers (strictly filtered from the shared network configuration
  collection by purpose).
- [x] API-key Integration catalogue reads for DPI applications and categories.
- [x] Formatting, strict Clippy, unit tests, release build, and live
  read-only-controller smoke tests.

## Network parity backlog

### N1 — remaining read-only Network batches (25 tools)

- [ ] **N1a — control-plane reads (7):** batch status, auto-backup settings,
  gateway settings, firewall-policy ordering, OON policy list/detail, and
  support-bundle status.
- [ ] **N1b — client analytics (4):** client statistics, sessions, Wi-Fi
  details, and per-client DPI traffic.
- [ ] **N1c — device/switch/radio diagnostics (9):** device radio data, LLDP
  neighbours, PDU outlets, port statistics, RF-scan results, speed-test
  status, switch capabilities, switch ports, and available channels.
- [ ] **N1d — events and traffic telemetry (5):** event types, IPS events,
  traffic-flow statistics, traffic flows, and event subscription.

Acceptance: each tool has an upstream-compatible name, schema, annotation,
success/error envelope, bounded output, unit coverage, and a read-only live
smoke test where the installed controller supports that endpoint. Integration
API routes must use the documented UUID site ID and preserve controller
`totalCount`/pagination metadata.

### N2 — safe client and device action parity (approximately 20 tools)

- [ ] Adopt, forget, rename, guest authorise/de-authorise, and static client IP.
- [ ] Force provision, locate, rename, LED control, outlet state, RF scan, and
  speed test.
- [ ] Add controller-version and capability checks for actions exposed by the
  Network 10.6 Integration API.

Acceptance: every mutation previews the exact target and request; `confirm`
is required; destructive/idempotency hints match upstream; no test confirms a
live action without an explicit operator request.

### N3 — configuration CRUD and write verification (approximately 72 tools)

- [ ] Create/update/delete/toggle WLANs, networks, port forwards, routes,
  user groups, port profiles, client groups, ACLs, QoS, traffic routes, firewall
  groups/zones/policies, DNS/Dynamic DNS, OON, content filters, and VPN state.
- [ ] Preview uses fetch-merge-write for partial updates.
- [ ] Post-write verification re-reads controller state and reports unapplied
  fields rather than calling a 200 response proof of success.
- [ ] Add write policy gates and permission-mode configuration compatible with
  upstream's confirmation model.

Acceptance: unit tests cover validation, no-op updates, policy denial,
preview, confirmed request shape, and failed post-write verification.

### N4 — complex Network writes and capability handling

- [ ] Firewall-policy ordering/reorder, gateway settings, SNMP, auto-backup,
  and backup lifecycle writes.
- [ ] Switch port aggregation, port mirroring, STP, jumbo frames, and PoE power
  cycle.
- [ ] Controller-version capability detection and actionable unsupported-feature
  responses across both legacy and Integration API routes.

Acceptance: API-key and session-auth requirements are explicit; controller ID
families cannot be silently mixed; destructive operations remain previewed.

## Product-server backlog

### P1 — Protect (62 tools)

- [ ] Create a separate Rust Protect MCP package/server.
- [ ] Port session authentication, cameras, events, recordings, detections,
  sensors, lights, chimes, live views, recognitions, alarm rules, and settings.
- [ ] Retain Protect-specific redaction and SuperAdmin alarm-manager handling.

### A1 — Access (37 tools)

- [ ] Create a separate Rust Access MCP package/server.
- [ ] Port doors, devices, credentials/PINs, schedules, policies, visitors,
  users, events, and system settings.
- [ ] Treat credential/PIN values as sensitive and require confirmation for all
  mutations.

## Deliberate exclusions until a supported contract exists

- [ ] UniFi Talk is **not** counted toward this reference parity target. The
  upstream project does not ship a Talk server, and the local Talk route tested
  by the original Rust scaffold returned the UniFi OS HTML portal rather than a
  JSON API response.

## Definition of done

- [ ] Every upstream Network, Protect, and Access tool is represented by its
  native Rust equivalent with matching name, input/output contract, and MCP
  annotations.
- [ ] All mutations use preview-confirm, policy gates, and post-write
  verification.
- [ ] API-key, local-session, and controller-version capability requirements
  are documented and tested.
- [ ] `cargo fmt --check`, strict Clippy, unit/integration tests, release build,
  and non-mutating live smoke tests pass.
- [ ] The tracker table reports 194/194, 62/62, and 37/37 exact-name parity.
