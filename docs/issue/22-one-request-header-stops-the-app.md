# 22 · one request header with no body stops the app: the declared length sizes the allocation

**Severity: high** — a single `Content-Length`, four header bytes of effort, and
the session is gone with no final flush. Reachable on the agent port (default
on, loopback) and on the sync port (default on, every interface, no credential —
see `21`).

## Evidence

`node oversize-probe.js <case> 1099511627776` against a sandbox session
(`ftest/oversize1/`). One request, 40 bytes of body, then a control ping:

```
sent one request declaring Content-Length: 1099511627776 (body was 40 bytes)
socket error: ECONNRESET
connection closed by peer; bytes received: 0
CONTROL AFTER: port refused the connection (ECONNREFUSED) — the session is gone
```

and the process's own two lines, in `console.log`:

```
quire: mcp server on 127.0.0.1:5880
memory allocation of 1099511627776 bytes failed
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

`quire.log` holds only `session started (pid 139532)` — no `session ended`, no
final flush record. This is the abort path `20` measured, and `C1` there is what
this costs: everything the session had recorded inside its quiet window dies
with it.

The same request on the sync port kills it identically
(`node syncprobe.js <case> <pid> abort`, `ftest/syabort2/`):

```
gate ok: :5878 belongs to pid 143140 (this harness' own session)
sent one request declaring Content-Length: 1099511627776 (body: none)
socket error: ECONNRESET
still alive? → listener gone
```

and `console.log` of that run carries the same `memory allocation of
1099511627776 bytes failed`. No token was needed for it.

**How much a declared size costs, measured** — `node memprobe.js <case>` keeps
connections open that announce a body and never send one, reading
`Get-Process -Id <pid>.WorkingSet64` between each (`ftest/mem3/memprobe.out`):

```
pid=171108 baseline WorkingSet=132196 KB
declared 64 MB, body not sent   → WorkingSet=197848 KB  (Δ 65652 KB, cumulative declared 64 MB)
declared 256 MB, body not sent  → WorkingSet=460028 KB  (Δ 327832 KB, cumulative declared 320 MB)
declared 1024 MB, body not sent → WorkingSet=1503272 KB (Δ 1371076 KB, cumulative declared 1344 MB)
after closing every connection  → WorkingSet=126964 KB  (Δ vs baseline -5232 KB)
session answers a normal request afterwards → 200 {"id":1,"jsonrpc":"2.0","result":{}}
```

The working set tracks the *declared* bytes at very nearly 1:1 — 1,344 MB
declared, 1,371 MB more resident — while zero body bytes have been sent. The
machine has 31.6 GB total and 12.1 GB free at the time of the run
(`Get-CimInstance Win32_OperatingSystem`), which is why the probe stopped at
1 GiB declared instead of pushing for the OOM; the 1 TiB arm is the arithmetic
end of the same line, and it is a *fault* rather than a refusal because Rust's
allocation failure is a `panic`/abort, not an error value.

## Cause

`src/mcp.rs:219-221` takes the length from the client, and `:243-244` sizes a
buffer from it before reading a byte of the body:

```rust
if name == "content-length" {
    content_length = value.parse().unwrap_or(0);
}
…
let mut body = vec![0u8; content_length];
reader.read_exact(&mut body)?;
```

Nothing bounds `content_length` (a 64-bit `usize` on x64 accepts
`1099511627776`), nothing caps the body size against the JSON-RPC payloads the
ten tools accept (the largest legal request this server can be asked for is a
5,000-character title plus a body), and there is no socket timeout anywhere in
`src/` (`grep set_read_timeout src/` → no hits), so `read_exact` waits forever
on a client that sends a header and no body. `serve` spawns one thread per
connection with no limit (`mcp.rs:186-196`), so N idle connections are N parked
buffers at once — the third row of the table above is three of them.

The sync server has the same two lines, in
`quire-core` `db0e95f` `src/services/sync/server.rs:57-62`:

```rust
content_length = v.trim().parse().unwrap_or(0);
…
let mut body = vec![0u8; content_length];
if content_length > 0 {
    reader.read_exact(&mut body)?;
}
```

with `0.0.0.0` at `:20` and no auth, which is what makes `21`'s chain a
remote-doS rather than a local one. The MCP stdio bridge repeats the pattern at
`mcp.rs:428` (`vec![0u8; length]`), with the smaller exposure of a parent process
that already owns the pipe.

`services/sync/transport.rs:25,29` does set 30-second read and write timeouts —
on the *client* half. The server half never asks.

## Candidate fixes

- Read into a bounded buffer instead of sizing from the header:
  `const MAX_BODY: usize = 8 * 1024 * 1024;` (or 1 MiB — the largest legal tool
  argument is far under it), then
  `if content_length > MAX_BODY { return respond(&stream, 413, …) }` **before**
  allocating, and stream the body with `Read::take(content_length as u64)`
  read-to-`Vec` after that check. Both `mcp.rs:243` and `server.rs:60` need it;
  the core one is a `quire-core` change.
- `stream.set_read_timeout(Some(Duration::from_secs(10)))` on the accepted
  socket in `handle_connection` (`mcp.rs:200`) and in `SyncServer::handle`
  (`server.rs:38`). That alone converts the parked-buffer rows above into closed
  connections.
- Answer `413` rather than dropping the connection, so a client that trips the
  cap learns why instead of seeing an `ECONNRESET` it will retry forever.
- Cap concurrency: the per-connection `std::thread::spawn` at `mcp.rs:194` is
  fine at ten and not fine at ten thousand. A `Semaphore`-shaped counter around
  the accept loop (an `AtomicUsize` and a refusal is enough — this codebase
  deliberately avoids new crates, see the dependency note at the top of
  `lan_server.rs`) bounds the worst case.
- The MCP page in 设置 already lists calls and their outcome; a rejected request
  that never reaches a tool has no line there. A `warn` into the log (and the
  bind-failure pattern of `main.rs:1143-1150`, "the bind runs here, on the
  thread that can still answer the UI") is where an operator would look.
- Note for whoever edits `mcp.rs`: the doc comment at `:166-167` still says
  "内嵌在运行中的应用里的 HTTP 服务。**默认关闭**" while `main.rs:1085`
  (`launch.mcp || state.setting_flag_or("mcp.enable", true)`) and
  `controller.rs:1683` make it default-on. A comment that understates the
  exposure is how a reviewer decides the body cap is not urgent.
