# Rust parity tracker

This is the visible implementation tracker for parity with the official local
UniFi Network API exposed by the user's Dream Machine Pro: **UniFi Network API
10.6.101**. The supplied documentation is the primary contract: API-key
authentication, UUID site IDs, documented request/response schemas, filtering,
pagination, and HTTP error envelopes.

The older compatibility-name manifest remains a secondary reference surface.
Its 194 names must not be mistaken for the complete 10.6.101 API contract.

## Current position

**Active priority: Network only.** Do not begin Protect or Access implementation
until the documented Network API 10.6.101 endpoint families are covered and the
Network definition of done below passes.

**Validation mode: full Network gate.** Static contract tests, unit tests, release builds, and non-mutating live smoke tests are required; no live mutation is confirmed automatically.

## Official API 10.6.101 baseline

The documented Network surface supplied for this controller comprises these
endpoint families:

- application information and local sites;
- adopted and pending devices, device details/statistics, and device/port
  actions;
- connected clients and client actions;
- networks and network references;
- Wi-Fi broadcasts;
- hotspot vouchers;
- firewall zones and policies;
- ACL rules;
- traffic-matching lists;
- WAN interfaces, site-to-site VPN tunnels, VPN servers, RADIUS profiles,
  device tags, DPI categories/applications, and countries.

The implementation tracker will report official-API endpoint coverage separately
from the secondary upstream-MCP name score below. An endpoint is only counted
as covered when its documented path, method, authentication requirement,
filter/pagination behaviour, schema, and error handling are implemented and
tested.

| Surface | Upstream tools | Rust tools | Exact-name parity | Status |
| --- | ---: | ---: | ---: | --- |
| Official Network API 10.6.101 | 73 documented operations | 73 catalogue operations | Path/method coverage implemented | **Primary target** |
| Secondary upstream Network names | 194 | 249 catalogue entries | 194/194 (100%) | Compatibility view |
| Protect | 62 | 0 | 0/62 | Not started |
| Access | 37 | 0 | 0/37 | Not started |
| Secondary upstream total names | 194 | 249 catalogue entries | 194/194 Network parity | Compatibility view |

`unifi_raw_network_endpoint` and the documented Integration API aliases are Rust-specific additions; they are not counted against the 194-name upstream Network contract.

The compatibility catalogue is split into `src/tools/compatibility.rs` and uses the checked-in upstream manifest for exact compatibility input schemas and annotation hints. Identified PUT updates use fetch-merge-write and post-write field verification; command-style routes report when stable read-back is unavailable. Every unsupported legacy route returns an explicit capability error. Site Manager connector reads are preferred when configured, with direct-controller fallback for read failures; mutations do not cross-retry after an ambiguous result.

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
- [x] Official API 10.6.101 batch 1: adopted devices, connected clients,
  networks, and Wi-Fi broadcasts (list/detail routes with documented UUID site
  routing and `totalCount` pagination).
- [x] Official API 10.6.101 batch 2: local sites, hotspot vouchers, traffic
  matching lists, firewall zones, and ACL inventory.
- [x] Official API 10.6.101 batch 3: WAN interfaces, site-to-site VPN tunnels,
  VPN servers, RADIUS profiles, device tags, and countries.
- [x] Official API 10.6.101 batch 4: application information, verified live
  against the controller-reported version `10.6.101`.
- [x] Official API write contract: preview/confirmation boundary for documented
  Network mutations, with network, Wi-Fi, voucher, firewall, ACL, and traffic
  matching-list operations wired to the Integration API request path.
- [x] Official API device/client action batch: adoption/removal, adopted-device
  actions, connected-client actions, and pending-device inventory wired through
  the same confirmed Integration API boundary.
- [x] Official API nested reads and query contract: adopted-device statistics,
  network references, firewall-policy ordering queries, and device-port action
  paths.
- [x] Official API ordering and filtered-delete writes: firewall-policy order,
  ACL order, and hotspot voucher deletion with query/body preview support.
- [x] Network compatibility diagnostics batch: client/device/session/DPI,
  event/IPS, RF/switch, traffic-flow, OON, and system read names routed through
  bounded legacy reads while the official API remains the primary contract.
- [x] Network compatibility action/WLAN aliases: adopt, force provision, port
  power-cycle, guest authorise/de-authorise, voucher revoke, and WLAN
  create/update/delete names map to documented Integration API operations.
- [x] Official Switching and DNS batches: switch stacks, MC-LAG domains, LAGs,
  and DNS policy list/detail/create/update/delete routes use the documented
  Integration API paths.
- [x] OpenAPI pagination contract: official list tools expose bounded `limit`
  and `offset` inputs and preserve the selected page in their response.
- [x] Formatting, strict Clippy, unit tests, release build, and live
  read-only-controller smoke tests. The latest matrix covered all 59 manifest
  read-only tools with no required inputs: 58 succeeded; batch status remains an
  explicit capability error because this controller exposes no batch-status route
  and this MCP executes batches synchronously.

## Network parity backlog

### N1 — legacy compatibility diagnostics (not part of the official API baseline)

- [~] **N1a — control-plane compatibility routes:** batch status, event subscription, alarm archiving, backup lifecycle, gateway/SNMP settings, and device control names are catalogued in the compatibility module; batch status remains unsupported because the controller exposes no route and this MCP executes batches synchronously.
- [x] **N1b — client analytics (4):** client statistics, sessions, Wi-Fi
  details, and per-client DPI traffic.
- [x] **N1c — device/switch/radio diagnostics (9):** device radio data, LLDP
  neighbours, PDU outlets, port statistics, RF-scan results, speed-test
  status, switch capabilities, switch ports, and available channels.
- [x] **N1d — events and traffic telemetry (5):** event types, IPS events,
  traffic-flow statistics, and traffic flows are catalogued and live-tested via
  current system-log and v2 equivalents; event subscription remains pending.

Acceptance: each tool has an upstream-compatible name, schema, annotation,
success/error envelope, bounded output, unit coverage, and a read-only live
smoke test where the installed controller supports that endpoint. Integration
API routes must use the documented UUID site ID and preserve controller
`totalCount`/pagination metadata.

### N2 — safe client and device action parity (approximately 20 tools)

- [~] Adopt, forget, rename, guest authorise/de-authorise, and static client IP names use manifest-backed schemas and preview-confirm enforcement; live route support remains controller-dependent.
- [~] Force provision, locate, rename, LED control, outlet state, RF scan, and
  speed test names use manifest-backed schemas and bounded request handling.
- [ ] Add controller-version and capability checks for actions exposed by the
  Network 10.6 Integration API.

Acceptance: every mutation previews the exact target and request; `confirm`
is required; destructive/idempotency hints match upstream; no test confirms a
live action without an explicit operator request.

### N3 — configuration CRUD and write verification (approximately 72 tools)

- [~] Compatibility CRUD names use manifest-backed schemas; identified PUT updates use fetch-merge-write and read-back mismatch reporting. Remaining controller-specific route behaviour is surfaced explicitly.
- [x] Preview and confirmed identified PUT updates use fetch-merge-write for partial updates.
- [x] Post-write verification re-reads identified PUT targets and reports unapplied fields; command-style operations explicitly report when stable read-back is unavailable.
- [ ] Add write policy gates and permission-mode configuration compatible with
  upstream's confirmation model.

Acceptance: unit tests cover validation, no-op updates, policy denial,
preview, confirmed request shape, and failed post-write verification.

### N4 — complex Network writes and capability handling

- [ ] Firewall-policy ordering/reorder, gateway settings, SNMP, auto-backup,
  and backup lifecycle writes.
- [ ] Switch port aggregation, port mirroring, STP, jumbo frames, and PoE power
  cycle.
- [x] Unsupported legacy and Integration API routes return actionable controller capability errors.

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
- [x] `cargo fmt --check`, strict Clippy, unit/integration tests, release build,
  and non-mutating live smoke tests pass for the local and Site Manager connector
  transports; confirmed writes remain excluded from live validation.
- [x] The Network tracker table reports 194/194 exact-name parity. Protect and Access remain explicitly out of this Network-only implementation scope.
