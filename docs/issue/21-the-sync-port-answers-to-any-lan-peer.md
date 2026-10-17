# 21 · the sync port answers to any peer on the LAN: whole library on a GET, notes deleted on a POST

**Severity: critical** — three facts, one chain. Every session listens on
`0.0.0.0:5878` with no credential; one UDP packet buys the right to push; and a
push that omits a collection deletes it. Measured end to end against a throwaway
`--db` in the sandbox copy, on loopback only.

## Evidence

`node syncprobe.js <case> <pid> info` against a session started with
`--db <case>/quire.db --auto-exit 90` (library `ftest/spx-info/`). The harness
refuses to touch the port unless `netstat` says the listener is the pid it
started, so what answered here is the sandbox session and nothing else:

```
gate ok: :5878 belongs to pid 151696 (this harness' own session)
GET /sync/info      → {"id":"windows-18df2afdf1dd8f60","name":"DESKTOP-HCPV6H6","kind":"windows","port":5878}
GET /sync/snapshot  → body=49247 bytes
   collections in the payload: {"pages":19,"blocks":145,"attachments":0,"databases":0,"notes":1,"tasks":0,"lists":0}
```

No `Authorization`, no token, no cookie — `GET /sync/snapshot` is the entire
library, page bodies included. Compare MCP, which requires the bearer token for
every request and refuses 401 without it (`17`–`19`'s harness proves both sides
of that).

Then `node syncpair.js <case> <pid>` (`ftest/sypair1/`), which first writes two
notes over MCP, sends **one** self-authored discovery datagram, and posts twice:

```
before: notes=2 ids=1,2 titles=["durab-164060-0","durab-164060-1"]
sent one discovery datagram claiming {"id":"lan-probe-1","name":"LAN probe 1","kind":"windows","port":5878}
POST /sync/snapshot (note 1 renamed)  → HTTP 200 OK
   read back: note "durab-164060-0" → "局域网改写的第一条" (notes=2)
POST /sync/snapshot (notes: [])       → HTTP 200 OK
   read back: notes=0 ids= trashed=0
```

and after the session was killed, read straight off the file
(`node q.js ftest/sypair1/snap/quire.db "select id,title,deleted_at from notes"`):

```
[]
```

The notes are not in the bin (`trashed=0` on the read-back) and not in the file.
The first POST was refused when it came from a device that had not announced
(`sp-wrap` run `ftest/spx-write/`):

```
POST /sync/snapshot (49239 bytes) →  LAN probe 推送了快照，但它从未在网络上广播过 — 已拒绝
```

so the gate exists — it is just that passing it costs one datagram.

## Cause

**The bind is on every interface, always.** `quire-core` `db0e95f`,
`src/services/sync/server.rs:19-21`:

```rust
pub fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("sync bind: {e}"))
}
```

and the shell starts it unconditionally — `src/app/controller.rs:257-274`,
"Start the engine and the pump. Called once from `wire`", with
`crate::services::sync::engine::Engine::start(…)` at `:274`; `Engine::start`
mounts the server (`engine.rs:194-206`) and the announcer/listener beside it.
There is no setting checked on that path, unlike MCP, which at least reads
`setting_flag_or("mcp.enable", true)` (`src/main.rs:1085`).

**No credential on any endpoint.** `server.rs:68-73` answers `GET /sync/info`,
`:74-79` answers `GET /sync/snapshot` by exporting the whole workspace, and
`:80+` accepts a POST — none of the three reads a header other than
`Content-Length`.

**Discovery is the pairing.** `server.rs:81-98` builds the `PeerRecord` from the
packet itself — `id`, `name`, `kind` from the body, `paired: true` from the
mere fact of arrival — and the refusal seen above comes from the engine checking
that this `device_id`/`ip` was heard on 5879. What it was heard *saying* is
unvalidated: `transport.rs:136-152`, `spawn_listener` binds `0.0.0.0:5879`,
takes the first 2048-byte datagram, and hands `(device, ip)` to
`Job::Discovered` for anything that parses as a `DeviceInfo` and is not this
device's own id. `DeviceInfo::from_json` has no field that could fail
verification, and `spawn_announcer` (`transport.rs:110-132`) shows the wire
format is a plain `info.to_json()` with no shared secret, nonce or signature.

**The delete rule.** `src/services/sync/model.rs:73-81` is explicit that an
*absent* collection is refused — "a payload that does not carry them is not a
payload with an empty organizer" — but those fields have no `#[serde(default)]`
only for absence, and `{"notes": []}` is presence with an empty list. Deletion
detection then turns that into `deleted` for every local row the payload does not
carry, which is what emptied the table. `version: 4` is the second lock on the
same door (a pre-organizer peer cannot send the field at all), and it holds
against an old client, not against a hand-written body.

## Candidate fixes

Ordered by what can ship without renegotiating the protocol:

- **Gate the endpoints on the pairing, not the IP.** The engine already knows
  which devices the user added (the 设备 list with its 配对 state). `POST
  /sync/snapshot` should require a peer the app has *paired*, and `GET
  /sync/snapshot` should require the same — today a stranger reads everything
  before asking anything. The minimum useful step is to make `PeerRecord.paired`
  come from the app's own pairing state instead of `: true` at `server.rs:93`.
- **Bind `127.0.0.1` unless the user said otherwise.** The sync *feature* is
  between the user's own devices, and every other Quire on the LAN has to be
  reachable to help — so the honest version is a setting (default off) that
  turns inbound sync on, mirroring how `lan.share` gates 5877
  (`main.rs:1057`, `if let Some(port) = share_port`). Today 5877 is behind a
  toggle and 5878 behind nothing.
- **A shared secret, even a weak one.** Both halves of the pair already write
  `sync.device-id` and can exchange a token the way MCP mints `mcp.token`
  (`mcp.rs:281` `write_pointer`). One `Authorization` header on the four endpoints
  and a matching `X-Quire-Sync` on discovery would make the announce-is-pairing
  path dead. This is a wire change, so it needs a version bump (`model.rs:61`
  `version`) and both shells to agree — the Android side is the second
  implementation and it is the reason this cannot be done quietly.
- **Refuse an empty collection unless the payload says so explicitly.** The rule
  the file already documents ("absent is not empty") should be carried one step
  further: a snapshot whose `notes`/`tasks`/`lists` are empty *and* whose device
  has no shadow yet is far more likely a mistake or an attack than a genuinely
  emptied organizer. A `{"organizer_empty": true}` flag, or requiring a
  tombstone count that matches, would have turned the second POST above into a
  refusal.
- **Say what a merge did.** After the second POST the app answered 200 with its
  own merged snapshot; the UI's 同步 log is the place for "本次合并删除了 2 条笔记",
  because right now the only way to discover the deletion is to look at the list.
- Tests: extend the sync integration suite with "a peer that announced itself
  over UDP but is not paired is refused" and "`{"notes": []}` from an unknown
  device does not delete local rows" — both are unit-testable against
  `SyncServer::handle` with a `Job` channel, no network needed.
