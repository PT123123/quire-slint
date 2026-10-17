# 24 · every sequential MCP call costs exactly the 250 ms pump period

**Severity: medium** — 4 writes per second, on an idle machine, for a client that
does the obvious thing: create, read the id, create the next. Nothing is broken;
the latency is structural, and it also stretches `20`'s unsaved window.

## Evidence

`node bulk.js <case> 20 0` — twenty `create_note` calls, no artificial gap, each
awaited (`ftest/dur-A1/bulk-marks.json`; `ms` is the response time of each call
from the start of the burst):

```
222, 472, 722, 973, 1223, 1474, … , 4482, 4733, 4982
wrote 20 notes in 5233 ms
```

The step is 250.0–250.5 ms, every time, on a machine doing nothing else. A
single call in a quiet session answers in 218–268 ms (`flushclock.js`:
`P1 create answered at +218ms`; `P2 … +227ms`; `flushclock2.js` first create
+266 ms).

The other half: **concurrency is fine**. `mcp-e2e.js` check `C1` fired twelve
creates at once:

```
C1  12 concurrent create_note calls → ids=9,10,11,12,13,14,15,16,17,18,19,20 unique=12  rows 7 → 19
```

All twelve land in the same pump tick — so it is not queue depth or a lock, and
not the HTTP thread either.

## Cause

`src/app/mcp_tools.rs:72-92`. The job queue is drained by one repeated Slint
timer, not by the arrival of a job:

```rust
t.start(
    slint::TimerMode::Repeated,
    Duration::from_millis(250),
    move || {
        …
        while let Ok(job) = rx.try_recv() {
            …
            let result = exec(&g, &s, &name, &args);
            …
            let _ = reply.send(result);
        }
    },
);
```

The HTTP thread's side blocks on that tick — `job_sender` at `:56-67` pushes the
job and then `reply_rx.recv_timeout(Duration::from_secs(120))`. So the floor on a
request is "until the next 250 ms boundary", which averages 125 ms and, for a
sequential client, locks onto 250 ms: the reply is produced *at* a tick, the
client's next request arrives a millisecond later, and it is therefore served at
the *following* tick. Exactly the cadence in the table, and the reason the
average is the worst case rather than half of it.

Two consequences beyond speed. The comment at `:70-71` explains the leak
(`漏掉的 Timer 会被 cancel，所以 Box::leak`) — the timer must outlive the wiring
scope, which is why it cannot be re-armed on demand. And a burst that never
stops arriving keeps `20`'s write queue open: at 250 ms per call the persistence
deadline is restarted faster than it can elapse, which is precisely the shape
`flushclock2.js` photographed (2.5 s burst, zero file writes until the end).

The engine's other pump is the same pattern with the same number, and the sync
side has the same shape (`crate::services::sync::engine` jobs are answered by
`ask()` at `quire-core` `server.rs:161-165` blocking on a UI-thread tick), so a
LAN peer's merge pays the same latency.

## Candidate fixes

- Wake the event loop when a job arrives. `slint::invoke_from_event_loop` is
  already the mechanism this codebase uses to cross into the loop from another
  thread (`src/main.rs:487`, the `--quit` path). In `job_sender`, after
  `tx.send(…)`, schedule a one-shot drain via `invoke_from_event_loop` and keep
  the 250 ms timer as the fallback for a missed wakeup. Sequential latency then
  drops to the work itself (single-digit ms for these tools), and the timer
  remains the safety net rather than the metronome.
- If the leak must stay exactly as it is, drain on demand *inside* the send:
  the HTTP thread can't touch `AppState`, so that path needs the pump to own a
  `Weak`-guarded re-arm. Not worth it next to the bullet above.
- Publish the limit where an agent reads it: `tool_defs()` descriptions are the
  only contract this server sends. A line in the MCP page of 设置 — "顺序调用约
  4 次/秒；并发调用不受此限" — is honest and cheap, and it tells the user why an
  import of 1,000 notes takes four minutes.
- Add a throughput check to the battery that fails on *regression* rather than
  on absolute speed: `10 sequential creates < 1.0 s` after the wakeup fix, or
  `< 3.0 s` while the timer is the only driver. The current harness would happily
  pass a 10× slowdown, because `C1` only checks uniqueness of ids.
