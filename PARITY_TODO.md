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
| Network | 194 | 68 | 67/194 (34.5%) | In progress |
| Protect | 62 | 0 | 0/62 | Not started |
| Access | 37 | 0 | 0/37 | Not started |
| Total | 293 | 68 | 67/293 (22.9%) | Not at parity |

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
- [x] Formatting, strict Clippy, unit tests, release build, and live
  read-only-controller smoke tests.

## Network parity backlog

### N1 — complete discovery, jobs, and diagnostic reads (53 tools)

- [ ] `unifi_load_tools` and `unifi_batch_status`.
- [ ] Event subscription, support bundle, event-type, IPS, session, traffic
  flow, client DPI/Wi-Fi/statistics, and device radio/RF/switch capability
  reads.
- [ ] Full list/detail coverage for ACLs, AP groups, client groups, content
  filters, DNS, Dynamic DNS, firewall policies/zones, OON, QoS, traffic routes,
  vouchers, VPN, and switch inventory.

Acceptance: each tool has an upstream-compatible name, schema, annotation,
success/error envelope, bounded output, unit coverage, and a read-only live
smoke test where the installed controller supports that endpoint.

### N2 — safe client and device action parity (16 tools)

- [ ] Adopt, forget, rename, guest authorise/de-authorise, and static client IP.
- [ ] Force provision, locate, rename, LED control, outlet state, RF scan, and
  speed test.

Acceptance: every mutation previews the exact target and request; `confirm`
is required; destructive/idempotency hints match upstream; no test confirms a
live action without an explicit operator request.

### N3 — configuration CRUD and write verification (75 tools)

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

### N4 — complex Network capabilities (4 tools)

- [ ] Firewall-policy ordering and reorder operation using the Integration API.
- [ ] Gateway settings, SNMP, auto-backup settings, and backup lifecycle.
- [ ] Switch port aggregation, port mirroring, STP, jumbo frames, and PoE power
  cycle.
- [ ] Controller-version capability detection and actionable unsupported-feature
  responses.

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
