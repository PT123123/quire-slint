# 37 · a declared body length is paid for in RAM before a byte of it arrives, and the answer goes back with the body still unread

**Severity: high** — both hand-written HTTP doors size a buffer from the client's
`Content-Length` and then wait for the bytes: `vec![0u8; content_length]` +
`read_exact` at `src/mcp.rs:243-244`, and the same two lines at
`quire-core@db0e95f src/services/sync/server.rs:60,62`. There is no cap and no read
deadline in either file (`grep set_read_timeout src/mcp.rs` finds nothing; the only
`Duration` in the file is a `sleep` in a test at `:555`). Measured on the MCP port:
one connection that announces a 200 MB body and sends **no body bytes** moves the
working set 129.6 MB → 315.3 MB in under eight seconds and holds it flat for 24 more;
eleven of them reach 411.2 MB and 35 threads. On the sync port the identical shape
needs **no credential at all** — three connections declaring 100 MB take 124.2 →
410.4 MB, +95.4 / +190.8 / +286.2 MB, one per connection, linear in the declared
number. And the other half of the same two functions: every refusal path (`401`,
`404`, `405`, and the `parse error` the mis-declared lengths get) answers and returns
**without consuming the bytes the client already sent**, so the client sometimes gets
an answer and sometimes gets an abort for the same request — twelve identical chunked
requests split 7 answered / 5 reset in the run whose log is on disk, and 9 / 3 in the
pass before that, so even the ratio is not a constant.

## Evidence

`bash mcpdrain.sh <mode>` — `ftest/mcpdrain.sh` + `ftest/mcpdrain.js`. Each mode
starts its own throwaway session (`--db <case>/quire.db --auto-exit 900`), reads the
Windows pid out of `quire.log`, refuses to speak to 5880 or 5878 unless the `netstat`
listener row names that pid, takes the token from the case directory's `mcp.json` and
never prints it. Logs: `ftest/mcpdrain-logs/<mode>.log`, and each mode's own session
artifacts (`quire.log`, `console.log`, `quit.out`, `quire.db`) in
`ftest/mcpdrain-logs/<mode>/`. **Every number in this record is transcribed from those
files**, from the one run of all six modes that kept them; an earlier pass had the
wrapper `rm -rf` its own case directory out from under its logs, and the two tables
that changed when the battery was re-run are called out where they appear (see the
last section).

### A. `headers` — what a framing mistake comes back as (5880)

```
[control-list]   HTTP/1.1 200 OK  3 ms    {"id":1,"jsonrpc":"2.0","result":{"tools":[{"descriptio
[D4-exact]       HTTP/1.1 200 OK  252 ms  {"id":1,…,"text":"{\"id\":1}","isError":false}
[D1-chunked]     ERR ECONNRESET   2 ms    (no bytes received)
[D2-boguscl]     HTTP/1.1 200 OK  1 ms    {"error":{"code":-32700,"message":"parse error"},"id":null}
[D3-short-decl]  HTTP/1.1 200 OK  1 ms    same
[D5-lowercase]   ERR ECONNRESET   1 ms    {"error":"POST only"}   ← bytes received *and* reset
[D6-dup-cl]      HTTP/1.1 200 OK  0 ms    same parse error
[D7-eof-midbody] (no response)    1 ms    response bytes=0
[D8-mixedcl]     HTTP/1.1 200 OK  244 ms  {"id":2,…}
[D9-uppercl]     HTTP/1.1 200 OK  251 ms  {"id":3,…}
```

and the file afterwards (`ftest/mcpdrain-logs/headers.log` tail, read out of the copied
`quire.db`): `pages=19 blocks=145 notes=3` — the three notes are `D4-exact`,
`D8-mixed-cl`, `D9-upper-cl`.

Read that list by what the file says, not by what the status line says:

- **Only three of the ten writes happened**, and they are exactly the three that were
  framed the way the server reads: `D4` with `Content-Length`, `D8` with
  `Content-length`, `D9` with `CONTENT-LENGTH`. Header *names* are matched
  case-insensitively here (`mcp.rs:217` lowercases every name before comparing) and
  that is correct — `23`'s lowercase-`bearer` finding is about the **value**, not the name.
- **`D1` — a request framed with `Transfer-Encoding: chunked` — gets a reset and no
  answer** (in this run; the same arm answered 7 times out of 12, see the last bullet,
  which is the point). The server never looks at that header: `content_length` stays `0`
  (`:208`), so `vec![0u8; 0]` succeeds, `read_exact` of nothing succeeds, and the body
  is `""` → `parse error`. The chunk bytes the client sent are still in the socket when
  the handler returns and drops it, which is what a reset is. An MCP client that uses
  chunked encoding (a documented option in Node's `http`, and what `curl -H
  "Transfer-Encoding: chunked"` does) cannot distinguish "refused" from "the app died".
- **`D2`/`D3`/`D6` answer `HTTP 200` with a JSON-RPC parse error and write nothing.**
  `Content-Length: notanumber` is `value.parse().unwrap_or(0)` (`:220`) — a value that
  cannot be read is silently read as "no body". `Content-Length: 5` against a 140-byte
  body reads 5 bytes and calls that the request. A duplicate header is a `HashMap`
  insert, so the last one wins (`D6` declared the true length and then `4`).
- **`D7` gets nothing.** Declared 100000, sent 60, then FIN: `read_exact` fails on EOF,
  `handle_connection` returns `Err`, and `serve` swallows it (`let _ = this…`, `:195`).
  Zero bytes in reply — not a `400`, not a JSON-RPC error.
- **The same request bytes produce two different outcomes, and the split moves between
  runs.** `race` fires twelve **sequential, byte-identical** requests (one socket closed
  before the next opens — `mcpdrain.js:302`) and tallies the status line:
  `chunked x12: 5x ERR ECONNRESET, 7x HTTP/1.1 200 OK` in the run on disk (`race`
  printed 3 / 9 in the pass before it, whose log the rig deleted). `CL=5 x12: 12x
  HTTP/1.1 200 OK` and
  `CL>sent then FIN x12: 12x (no response)` — those two are stable. So the non-determinism
  is not a concurrency artefact: the same bytes, one at a time, land on two different
  transports. And `D5` is the clearest single line in the table: the client received
  `{"error":"POST only"}` **and** reported `ECONNRESET`, so "answered" and "aborted" are
  not two outcomes but one outcome a client may report either way.

### B. `park` — what a declared length costs before any body byte exists (5880)

```
baseline: ws=129.6MB private=83.9MB threads=24
one connection declaring a 200 MB body and sending none of it:
  t=   7.7s ws=315.3MB private=269.9MB threads=25  control tools/list [HTTP/1.1 200 OK] 4 ms
  t=  13.4s ws=315.3MB private=269.9MB threads=25  control [200] 3 ms
  t=  19.2s ws=315.3MB private=269.9MB threads=25  control [200] 2 ms
  t=  24.9s ws=315.3MB private=269.9MB threads=25  control [200] 3 ms
now ten more, each declaring 10 MB:
  t=  31.7s 11 parked: ws=411.2MB private=366.4MB threads=35
  control call while parked: [HTTP/1.1 200 OK] 2 ms
closing every parked connection from the client side:
  t=  36.5s after close: ws=125.0MB private=79.3MB threads=24
  t=  52.3s after close: ws=125.0MB private=79.3MB threads=24
```

+185.7 MB for a 200 MB declaration — the buffer is not only allocated, it is
zero-filled (`vec![0u8; n]`), so the pages are **touched** and resident, not merely
reserved. Flat across the four samples 24 s apart: nothing reaps it, nothing shrinks
it, and it is still 125.0 MB seventeen seconds later with nothing to release. Eleven
connections, eleven more threads (24 → 35), and the app answers a real request in 2 ms
the whole time. Closing the clients takes all of it back.

### C. `authorder` — the credential does gate the memory on 5880 (and that is the good news)

Each arm declares the same 200 MB body and sends none of it:

```
baseline: ws=123.9MB private=78.4MB threads=24
[no Authorization header]  Δws=0.0MB    threads=24  answer=[HTTP/1.1 401 Unauthorized]       fin=true  bytes=124
[wrong bearer token]       Δws=0.0MB    threads=24  answer=[HTTP/1.1 401 Unauthorized]       fin=true  bytes=124
[right token, GET]         Δws=0.0MB    threads=24  answer=[HTTP/1.1 405 Method Not Allowed] fin=true  bytes=127
[right token, POST /nope]  Δws=0.0MB    threads=24  answer=[HTTP/1.1 404 Not Found]          fin=true  bytes=118
[right token in X-Quire-Token] Δws=+190.8MB threads=25 answer=(nothing received yet)         fin=false bytes=0
[right bearer, POST /mcp]      Δws=+381.5MB threads=26 answer=(nothing received yet)         fin=false bytes=0
after closing all six: ws=123.9MB private=78.4MB threads=24
```

Sampled 3.5 s after each arm. The four refusals move the working set by **0.0 MB** —
not "a little", not a settling artefact, nothing — and every one of them gets a complete
response with `fin=true`. The order in the source is what protects the port: `404` at
`:232-234`, `401` at `:235-237`, `405` at `:238-242`, and only then `:243`. Each accepted
credential (`Authorization: Bearer`, or `X-Quire-Token`, `:225-229) buys its own ~191 MB
and they stack, because the buffer belongs to the connection's thread, not to the port.

### D. `deadline` — there is no read timeout, only the client's goodwill

```
baseline: ws=129.6MB private=84.3MB threads=24
  t=  33.0s ws=124.5MB threads=25 (+1 over baseline)   control [HTTP/1.1 200 OK] 2 ms
  t=  63.8s ws=124.4MB threads=20 (-4)                  control [HTTP/1.1 200 OK] 2 ms
  t=  94.5s ws=124.4MB threads=20 (-4)                  control [HTTP/1.1 200 OK] 2 ms
  parked request ended 92337 ms after it was sent, with: (socket closed with no response)
  t= 101.3s (6 s after the client closed): ws=124.4MB threads=19
```

A connection that sent `Content-Length: 100` and 10 body bytes sits for 92.3 seconds
(1 min 32 s, sampled at 30 s intervals until the client gave up) with no answer, no
error, and no reaping, and is disposed of only when the **client** hangs up. This arm
declares 100 bytes, not 200 MB, so it costs no memory — it measures the missing deadline
alone. (Thread counts drift on their own — 24 → 25 → 20 → 19 in this run as the UI's
workers come and go — so only the per-arm deltas and B's monotone 24→25→…→35 are
quoted.)

### E. `sync` — the same transport, on the door with no credential

`0.0.0.0:5878`, gated on this session's pid, no token involved:

```
[control GET /sync/info]  HTTP/1.0 200 OK  3 ms  bytes=185
[POST snapshot, "Content-Length: 14"]   400  bad snapshot: missing field `version` at line 1 column 14
[POST snapshot, "content-length: 14"]   400  bad snapshot: missing field `version` at line 1 column 14
[POST snapshot, "Content-length: 14"]   400  bad snapshot: EOF while parsing a value at line 1 column 0
[POST snapshot, "CONTENT-LENGTH: 14"]   400  bad snapshot: EOF while parsing a value at line 1 column 0
[POST snapshot, "X-Content-Length: 14"] 400  bad snapshot: EOF while parsing a value at line 1 column 0
[POST snapshot, chunked]                400  bad snapshot: EOF while parsing a value at line 1 column 0
memory baseline: ws=124.2MB private=78.7MB threads=24
  1 parked (declares 100 MB, sends nothing): ws=219.6MB private=174.3MB threads=25  Δws=+95.4MB  answer=(nothing yet) fin=false
  2 parked:                                     ws=315.0MB private=269.9MB threads=26  Δws=+190.8MB
  3 parked:                                     ws=410.4MB private=365.5MB threads=27  Δws=+286.2MB
  control while parked: HTTP/1.0 200 OK 2 ms
  after closing: ws=124.3MB private=78.7MB threads=24
```

Two findings in six lines of that table:

- **The header name is matched with two exact `strip_prefix` calls** —
  `"Content-Length:"` and `"content-length:"` (`server.rs:54-55`) — so `Content-length:`
  and `CONTENT-LENGTH:` are **not** recognised, and the 14 bytes the peer sent become an
  empty body. The same request, four bytes of capitalisation apart, gets two different
  answers. HTTP field names are case-insensitive; the MCP door gets this right by
  lowercasing every name (`mcp.rs:217`), and this door re-implements the parser without
  that line.
- **The memory arm needs nothing.** Three sockets, no credential, +286.2 MB resident in
  seven seconds, in a process that is bound to every interface (`server.rs:20`) and
  answers to any LAN peer (that is `21`, and `29` is the firewall policy that lets the
  question be asked from another machine at all). `22` measured the arithmetic end of
  this line — one `Content-Length: 1099511627776` and the process dies on the failed
  allocation with no `session ended`. What this adds is the middle of the line: no
  abort, no refusal, just resident bytes for as long as the peer keeps its sockets open.
- **What moved when the battery was re-run, and what did not.** The earlier pass had
  `X-Content-Length: 14` and `chunked` on this port come back `ECONNRESET`; in the run
  on disk all seven arms answer `400` with a body. So the reset claim is **not** made
  about 5878 — it is made about 5880, where `race` measured it twice and got a split
  both times. The header-case finding (two different serde messages four bytes of
  capitalisation apart) and the linear memory did reproduce exactly.

## Cause

Two hand-written HTTP parsers, four shared omissions.

- **The length is trusted before the bytes are read.** `src/mcp.rs:208` initialises
  `content_length = 0`, `:219-221` takes the value from the header with
  `value.parse().unwrap_or(0)`, `:243` sizes the buffer, `:244` waits. Quire-core
  duplicates it at `services/sync/server.rs:45,54-57,60,62`. Neither caps it; neither
  streams into a bounded `Vec`.
- **No deadline.** Neither file sets `set_read_timeout`, so `read_exact` waits for the
  peer's goodwill (D: 92 s and counting) and the only reap is the client closing.
- **No drain.** `respond` (`mcp.rs:253-269`) writes `Connection: close` and the handler
  returns; the bytes the client already sent are never consumed. *This is my reading of
  why a reset happens rather than a measurement of Winsock's rule* — the measurement is
  simply that twelve identical requests split 7 answered / 5 reset in the run on disk
  (9 / 3 in the pass before it), that the three arms whose bodies the server did consume
  (`D4`, `D8`, `D9`) never reset, and that `D5` received the full `405` response body
  *and* a reset.
- **One thread per connection, unbounded, with a clone per connection.**
  `mcp.rs:185-198` and `server.rs:25-37` both `std::thread::spawn` per accepted socket,
  and the MCP one rebuilds `McpServer { token, tools, call }` (`:189-193`) — ten tool
  definitions and a token string — for every connection, parked or not.
- The two doors are *not* the same code: `mcp.rs` lowercases header names and
  `server.rs` does not, and `mcp.rs` answers `HTTP/1.1` where `server.rs` answers
  `HTTP/1.0` (`server.rs:182`). Fixing one does not fix the other, and they live in
  different repositories.

## What this is not

- **Not request smuggling.** One request per connection, `Connection: close` in every
  response, and the socket is dropped after the reply, so an unread body cannot be
  mistaken for the next request. The duplicate-`Content-Length` arm is a correctness
  wart, not a desync.
- **Not a leak.** Every arm returns to baseline when the client hangs up — 411.2 →
  125.0 MB and 35 → 24 threads in B, 505.4 → 123.9 MB and 26 → 24 in C, 410.4 → 124.3
  MB and 27 → 24 in E.
- **Not a hang.** `tools/list` answered in 2–4 ms with eleven connections parked, and
  `--quit` was graceful in **all six modes** of the run whose artifacts are on disk
  (`/tmp/mcpdrain-all.log` and `ftest/mcpdrain-logs/<mode>/`): `rc=0`, `quire: the
  running instance accepted the quit request`, `alive_after_quit=no`,
  `ended_line=1` (one `session ended` line in `quire.log`) six times out of six.
- **Not reachable unauthenticated on 5880.** Measured in C: 401/404/405 all return
  before the allocation. It is reachable unauthenticated on 5878, which is `21`'s
  finding about that port, now with a second thing a peer can do there.
- **Not a silent sync door.** `bad snapshot: missing field `version` at line 1 column
  14` is a precise, useful refusal. The complaint is about which bytes reach the parser,
  not about what it says when they do not make sense.

## Candidate fixes

In rough order of lines-touched-per-harm-prevented:

- **Cap the declaration before allocating it.** One comparison at `mcp.rs:243` and at
  `server.rs:60`: `const MAX_BODY: usize = 8 * 1024 * 1024;` → answer `413` (MCP) or
  `400 body too large` (sync) and return. The 200 MB declaration in B becomes a refusal
  that moves nothing.
- **Set a read deadline.** `stream.set_read_timeout(Some(Duration::from_secs(10)))?`
  immediately after accept (`mcp.rs:200`, `server.rs:38`). Nothing else about the shape
  has to change: the `Err` already propagates out of `handle_connection`, and
  `serve`'s `let _ =` is already the right response to it.
- **Stream instead of pre-allocating.** `let mut body = Vec::with_capacity(n.min(65536));
  reader.take(n as u64).read_to_end(&mut body)?` — never touches more pages than have
  arrived. The pattern already exists in this codebase, on the client side:
  `services/sync/transport.rs:93-101` reads with `take`/`read_to_end`.
- **Match header names the way the protocol says.** Delete the two `strip_prefix` arms at
  `server.rs:54-55` and do what `mcp.rs:217` does — `line.split_once(':')`, lowercase the
  name, then compare. One line, and `Content-length:` stops being a body that vanishes.
- **Say something about the framing you do not support.** `Transfer-Encoding: chunked`
  should be answered `411 Length Required` (or implemented), a non-numeric
  `Content-Length` should be `400`, not `0`, and a `read_exact` that hits EOF should
  answer `400 incomplete body` instead of dropping the connection silently. Every one of
  those is a `return respond(...)` on a line that currently returns `?`.
- **Drain or refuse explicitly.** If a refusal path is going to close without reading the
  body, do it in a way the client can tell the difference: answer, then
  `stream.shutdown(Shutdown::Both)`, or read-and-discard up to the cap first. The
  reliable half of this is the cap + the drain; a delay is a workaround.
- **Bound the concurrency.** A semaphore around the accept loop, or a small pool; and
  share `Rc<McpServer>` instead of rebuilding `token` + `tools` per connection at
  `mcp.rs:189-193`.
- **Land it twice.** `mcp.rs` and `quire-core`'s `services/sync/server.rs` need the same
  change, and `services/lan_server.rs:55-61` needs the deadline and the header-name
  treatment even though it never reads a body.
- **Pin it with a gate.** The six modes of `mcpdrain.js` are the gate, and the fix
  version is checkable: `D8`/`D9`/`D4` still write rows; `D1` answers `411` instead of
  resetting; `D2` answers `400`; a 200 MB declaration moves the working set by < 5 MB
  and answers in under 10 s; the three-case header arms on 5878 give the same serde
  message; and `chunked x12` reports twelve of one outcome instead of the 7 + 5 (and 9 +
  3) measured here.

## Not measured, said plainly

- **Every arm came from `127.0.0.1`.** The credential-free reachability is
  `server.rs:20`'s `0.0.0.0` bind plus `21` (that port answers any peer) plus `29`
  (the firewall rules that let it), not a handshake from a second machine.
- **The far end of the allocation line is `22`'s**, not re-run here: the 1 TiB
  declaration that aborts the process. This record measured up to 505.4 MB resident (C,
  two parked connections) and stopped because the shape was proven and the machine is
  the user's.
- **The reset mechanism is an interpretation.** What is measured is the split (7 / 5 for
  identical bytes in the run on disk) and that the arms whose bodies were consumed did
  not reset.
- **The `9 / 3` and the two 5878 resets come from a pass whose log no longer exists.**
  The rig deleted its own evidence (`ftest/mcpdrain/` was `rm -rf`'d at the start of
  every mode while the wrapper tee'd into it); those numbers are quoted only where the
  re-run disagreed with them, and no claim in this record rests on them alone.
- **Thread-count baseline drift** (24 → 20 → 19 inside one session) means only
  same-run deltas are quoted; nothing here claims a thread *leak*.
- **Body size through the tool surface is untested** — a 10 MB `body` argument in
  `create_note` was not driven, so no claim is made about where that limit is, if any.
- **5877 is not implicated in the allocation.** `lan_server.rs` reads a request line and
  headers and never a body, so B and E do not transfer to it; its POST-with-body framing
  was not re-measured (`28` measured its `405`s, not its resets).
- **No pixel claim anywhere.** This is a socket-and-RSS record.

## How this harness got it wrong

1. **A paused socket does not report a remote close.** The first `authorder` run
   printed "connection still open, server waiting" for four arms the server had
   answered and hung up on 3.5 seconds earlier — the helper resolved on `connect` and
   attached no `data` listener, so Node paused the readable side, never emitted `end`
   or `close`, and the flag stayed false. Fixed by tracking `data`/`end` per socket and
   re-running; the corrected run shows `401` (124 bytes), `405` (127), `404` (118) with
   `fin=true` for exactly those four, and `bytes=0, fin=false` only for the two that
   really are parked. Had I not caught it, the record would have claimed the memory was
   reachable without a token — which C disproves.
2. **A promise that never settles makes Node exit silently.** `raw()` resolved only on
   timeout, `end` or `error`; when the deadline arm destroyed its own socket the promise
   stayed pending, the event loop went empty, and the process exited — printing the
   three samples and skipping the teardown, so the log ended mid-mode with no `done`.
   The 92.3 s figure exists because `close` was added as a resolving event and the arm
   re-run.
3. **The wrapper never trusts `$!`.** The Windows pid comes from `quire.log`'s
   `session started (pid N)` line and is cross-checked against the `netstat` listener
   row for 5880 before any byte is sent (`lan.sh`'s shape, copied for the same reason
   `14` documents it), and each mode waits for 5880 to be free before starting.
4. **The rig deleted the evidence for four of its six tables.** `mcpdrain.sh` starts
   with `rm -rf "$CASE"` and the first version tee'd its log *inside* `$CASE`, so every
   mode wiped the previous mode's output and the last mode's wiped itself; only
   `mcpdrain-headers.log` survived, and it survived outside the case directory by
   accident. The wrapper now writes `ftest/mcpdrain-logs/<mode>.log` and copies
   `quire.log`/`console.log`/`quit.out`/`quire.db` per mode, and the battery was re-run
   end to end. That re-run is why this record says what it says about 5878's resets: two
   arms had reset in the wiped pass and answered `400` in the kept one, and the chunked
   split moved. A table with no artifact on disk behind it is a draft, not evidence —
   `verify-harness-before-trusting-failure` in the notes says to check the fixture before
   the finding, and here the fixture was eating its own output.
