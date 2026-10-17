# 41 · the sync port hands out every attachment's bytes to any peer, one id at a time — and each request writes into the user's own log

**Severity: critical** — `sync/server.rs:112-119` answers `GET /sync/attachment/<id>` on the
always-on `0.0.0.0:5878` with no credential and no pairing check, and `controller.rs:699-726`
reads the file out of `<library>/attachments/` and sends it. Measured against this harness'
own session: `GET /sync/snapshot` returned 50,192 bytes naming 3 attachments with their ids,
labels, MIME types, exact byte counts **and on-disk file names** (`1:sample-gradient:image/png:146748:1.png …`),
and then three plain GETs returned bodies whose md5s equal the source files byte for byte —
`f91753f50415`, `74ca03f49398`, `f91753f50415`. Ten repeats of the 1.8 MB one cost 534 ms each,
identical every time, with no token, no rate limit and no session. This is the media half of
`21`: that record proved the library's *text* leaves the machine; this one proves the pictures
and the attached files do too. It comes with a second, smaller finding: all six of the junk ids
that answered `404` appended a line to the persisted `sync.log` settings row, so an
unauthenticated reader can also write to the audit trail the user is meant to read.

## Evidence

`bash attsync.sh` — `ftest/attsync.sh`, `ftest/getbin.js`, kept at `ftest/attsync.log` and
`ftest/attsync/` (`fetch/att-1.bin` … `att-3.bin`, `fetch/snapshot.json`, `snap/quire.db`).
The library is the copy leg A of `40` built (three attachment rows and three files), placed in
a fresh case directory with its `attachments/` folder; the session ran with no `--share`, no
flag of any kind, and `--auto-exit 900`. 5878 was gated on the listener pid belonging to that
session (`session pid=145396`, `5878 listener check 145396`) before a byte was sent, and
`%APPDATA%\Quire\quire.db` was never opened. `getbin.js` is a binary-safe GET because
`getfile.js` does `buf += d` — utf-8-decoding a PNG on the way in and destroying the very
comparison this arm makes.

### A. What the port announces before being asked for anything

```
GET /sync/info       -> 200, application/octet-stream, 87 B
    {"id":"windows-18df380e3d1e11b8","name":"DESKTOP-HCPV6H6","kind":"windows","port":5878}
GET /sync/snapshot   -> 200, application/octet-stream, 50192 B
    keys: version,device_id,device,pages,blocks,attachments,databases,notes,tasks,lists
    attachments=3
      1:sample-gradient:image/png:146748:1.png
      2:quarterly-report.pdf:application/pdf:1842000:2.pdf
      3:sample-gradient:image/png:146748:3.png
    pages=19 blocks=148 notes=0
```

`blocks=148` is 145 seed blocks plus the three attachment rows from `40`'s leg A, so this is the
same library, not a fresh one. The snapshot needs no pairing — `server.rs:71-73` answers
`GET /sync/snapshot` by handing the request to `Job::ExportSnapshot` and replying with the
result. `SAttachment` carries `file` (`migrations.rs:189-198`'s column) all the way to the wire,
so a reader learns the folder's file names and the exact sizes without fetching anything.

### B. The bytes

```
source folder: 1.png=146748 B  2.pdf=1842000 B  3.png=146748 B
source md5:    1.png=f91753f50415  2.pdf=74ca03f49398  3.png=f91753f50415

GET /sync/attachment/1 -> 200, 146748 B, md5=f91753f50415
GET /sync/attachment/2 -> 200, 1842000 B, md5=74ca03f49398
GET /sync/attachment/3 -> 200, 146748 B, md5=f91753f50415

1.png == fetched 1 ? yes     2.pdf == fetched 2 ? yes     3.png == fetched 3 ? yes
```

Nothing was needed to obtain them: no `POST /sync/pair`, no token (there is no token anywhere on
this port — `21`), no session, no Referer, no ordering. The ids came from arm A, and the route's
own index page (`GET /` answers the five-route table at `server.rs:141`) says `GET
/sync/attachment/<id> one attachment's bytes`.

### C. Walking it

```
10 x 1842000 B in 5346 ms = 534 ms each, 17 MB moved
distinct files fetched: 1 (1 = same bytes every time)
```

Each request is its own connection and its own thread (`serve` spawns per connection, and
`AttachmentBytes` does a fresh `fs::read` every time — `controller.rs:707-708`), so a peer pulls
the folder repeatedly at ~3.5 MB/s from this box. The id is not consumed, cached or marked.

### D. The misses write into the victim's own library

Six arms for ids that are not rows — `0`, `999`, `1.png`, `''`, `-1`,
`99999999999999999999` — all answered `HTTP/1.0 404`, 18 B (`no such attachment`). Every one of
them also produced a log line, and that log is a settings row:

```
select key,length(value) n from settings where key like 'sync%':
  sync.device-id 24   sync.device-name 15   sync.log 519

select value from settings where key='sync.log':
  [{"at":"2026-10-17 05:39","peer":"对端","ok":false,"message":"本机没有附件 0 的文件 — 对端本轮同步会失败"},
   {"at":"2026-10-17 05:39","peer":"对端","ok":false,"message":"本机没有附件 999 的文件 — 对端本轮同步会失败"},
   … six lines, all 05:39 …]
```

That row is still in the copied library after `--quit rc=0 survived=no ended_lines=1`, so the
write is durable. `sync_log_push` (`state.rs:15529-15541`) pushes into a 50-line tail and stores
it with `record_setting("sync.log", …)`; the miss path that calls it is deliberate and commented
(`controller.rs:716-724`, "the row is this device's own book, so the line has to be said here").
Two consequences follow from its shape: the `peer` field is the constant `"对端"` — "the other
side" — so six lines written by an anonymous network reader are indistinguishable from six lines
written by a real paired laptop; and `at` is `now_rfc3339`, which formats to **minute**
resolution (`engine.rs:168-172`), so all six requests collapse into one timestamp and cannot be
ordered even against each other.

## Cause

- `handle` matches five routes (`server.rs:67-141`) and checks no credential on any of them.
  `POST /sync/snapshot` goes so far as to *invent* a paired peer record for whoever sent the
  body (`:84-96`, `paired: true`), so the model "we only talk to paired devices" is not enforced
  anywhere on the socket.
- The attachment route was written for a symmetric protocol: `engine.rs:336-345` fetches
  `/sync/attachment/<id>` only for ids the local device lacks, so the server side never needed
  to ask "should this requester have it". `ask()` (`server.rs:117`) forwards the id straight to
  the UI-thread job, which reads `display_path(att)` then `stored_path(att)` off disk
  (`controller.rs:699-726`) and replies.
- The id is `strip_prefix(…).and_then(parse::<u64>).unwrap_or(0)` (`:113-116`), which is why the
  junk arms are safe — `1.png`, `-1` and the 20-digit number all become id `0`, and there is no
  row 0. That is the *one* thing in this path that is checked.
- The log write is a `record_setting`, i.e. the same debounced settings pipeline every MCP call
  touches — `20` measured that a read which writes a settings row keeps the flush queue open;
  here an unauthenticated network read dirties the library and persists a user-visible line.

## What this is not

- **Not a write channel.** Every arm in this record was a `GET`; the three media files kept
  their md5s, `pages`/`blocks`/`attachments` counts did not move, and the only row that changed
  was `sync.log`. (The mutating arms are `21`'s: `POST /sync/snapshot` with `{"notes": []}`.)
- **Not a path traversal.** The path is `dir.join(&att.file)` where `att.file` came from *this*
  device's own row — the writer is `attachment_store.rs:108` / `:216`, which mints `{id}.{ext}` —
  and the id is parsed as `u64`. Measured six ways in D. The place where a remote `file` string
  *would* be joined is the receiving half, and that half sanitises on purpose
  (`state.rs:15203-15224`: "the peer's own `file` and `thumb` are network input, and joining them
  onto the attachments folder writes at whatever path they hold").
- **Not the 5877 share.** The read-only share has no attachment route at all — four shapes
  answered `not found` in `40`'s leg B — so this is specifically the sync port, which unlike
  5877 is behind no toggle at all (`21`: "Today 5877 is behind a toggle and 5878 behind nothing",
  and the `paired: true` at `server.rs:93` is the reason the port cannot tell one peer from
  another).
- **Not a second machine's observation.** Everything came from `127.0.0.1` on this box; "any LAN
  peer" rests on the measured `0.0.0.0` bind plus `29`'s 130 inbound firewall rules, same as `21`.

## Candidate fixes

1. **Put a credential on the sync port, or make the pairing real.** `POST /sync/pair` already
   exists; the cheapest gate is a device token minted at pairing and required on
   `/sync/snapshot`, `/sync/attachment/…` and `POST /sync/snapshot` — and stop fabricating
   `paired: true` at `server.rs:93`. Anything smaller is a filter over an open door.
2. **Bind to the session that asked.** `Job::ListLocalAttachments` + `engine.rs:336` mean the
   only legitimate requester is a device that just announced a snapshot and named the ids it
   lacks; the server could answer `/sync/attachment/<id>` only for ids in that peer's current
   missing-set, which makes an id walk impossible without inventing an auth system.
3. **Stop shipping `file` in the snapshot.** The receiver rebuilds the name locally
   (`state.rs:15214-15218`) and never uses the sender's; leaving it in `SAttachment` costs a
   name the reader has no use for and gives `21`'s exfiltration a folder listing for free.
4. **Unauthenticated traffic must not write the user's audit log.** Keep the miss line in memory
   (it exists to explain a failed round to *this* device's own cycle) and drop the
   `record_setting`; or persist it only for a paired device. Either way, name the peer instead of
   the constant `"对端"` and give `now_rfc3339` seconds (`engine.rs:168-172`), because a log with
   minute resolution and no subject is neither an order nor an audit.
5. **A gate that fails today:** from an unpaired socket, `GET /sync/attachment/<known id>` must
   answer 4xx; and a six-arm junk battery must leave `select length(value) from settings where
   key='sync.log'` unchanged.

## Not measured

- **The downscaled case.** `Job::AttachmentBytes` reads `display_path` first (`controller.rs:707`),
  which for a picture over `MAX_EDGE = 1280` is the `.cache.png` (`attachment_store.rs:114-131`).
  All three fixtures here are 640×400 with `thumb = ''`, so display and stored are the same file
  and the two md5 arms cannot tell them apart. Whether a peer can ever *not* get the original is
  therefore unproven, and so is the reverse — that it can.
- **A push.** Every byte here moved in reply to a GET from this harness. Nothing measured whether
  a victim session ever sends attachment bytes unprompted, or what a real two-session merge does
  with the same files. `38`'s "the sync door carries media" claim stands on reading
  `state.rs:15938-15950`, not on a second device.
- **Cost at size.** Three files, 2.1 MB total. A library with a folder of gigabyte attachments
  was never walked, so the ~3.5 MB/s figure is for one 1.8 MB file repeated ten times, and the
  thread-per-connection cost of a concurrent walk is unmeasured (`37` measured the allocation
  side of that on other ports).
- **The pairing UI**, the 「淘汰离线设备」 button, `PEER_STALE_SECS` pruning
  (`state.rs:15180-15186`) and the 5879 announcer were not driven here.
- **No pixel claim.** Nothing was photographed; every number is a `node:sqlite` read or a raw
  HTTP/1.0 response.

## How the harness got it wrong first

- The first pass used `getfile.js` and reported `1.png == fetched 1 ? no` with a mangled length.
  `getfile.js` builds a string out of the socket (`buf += d`), which is correct for the markdown
  arms it was written for and destructive for a PNG. `getbin.js` keeps `Buffer`s and cuts the
  header off in bytes; the md5 equality above is from that.
- `grep -c '本机没有附件' quire.log` answered 0 and looked like the miss path never spoke. It does
  speak — to the sync log, not to the session log — and the six lines are only visible through
  `settings`. Reading the wrong file would have turned D into a clean result.
- The 5878 PID gate is the reason this record has three attachments to fetch: had a foreign
  session been listening, the snapshot would have been the *user's* library and every number
  above would have been a disclosure the harness caused.
