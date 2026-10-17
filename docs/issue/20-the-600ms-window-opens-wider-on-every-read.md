# 20 · a session that dies 0 ms after its last write loses all of it — and a read keeps that window open

**Severity: high** — the loss window is real, it is 600 ms wide, and the thing
that re-opens it is not a write. An agent polling the library every few hundred
milliseconds holds the whole session's edits in memory for as long as it likes,
with nothing on screen saying so.

## Evidence

**The quiet period, measured on the file.** `.scratch/issue-test/ftest/flushclock.js`
writes one note over MCP and then does nothing but `stat` the database trio every
20 ms (library `ftest/fclock5/`):

```
P1 create answered at +218ms
+823ms P1: quire.db-wal 766352 -> 774592      ← 605 ms after the write
P2 create answered at +227ms, read answered at +477ms
+1086ms P2: quire.db-wal 774592 -> 782832     ← 609 ms after the *read*
```

**A burst never writes mid-burst.** `flushclock2.js`, ten creates, file sampled
throughout (`ftest/fc2a/`):

```
+266ms create #0 answered      +1519ms create #5 answered
+516ms create #1 answered      +1770ms create #6 answered
+767ms create #2 answered      +2021ms create #7 answered
+1017ms create #3 answered     +2271ms create #8 answered
+1268ms create #4 answered     +2522ms create #9 answered
                               +3148ms wal 766352 -> 774592   ← one write, 626 ms after the last
```

Nothing on disk moves for the entire 2.5 s. All ten notes live in a `Vec` in the
process.

**What dying inside that window costs.** `durab3.js` + `durab3.sh`: ten notes,
then the abort request *from the same process*, so the distance from the last
write is a number rather than a guess, and the file is read from a snapshot copy
after the process is gone (no new session, which could write back what it
recovered and blur the answer):

```
C1  n=10  wait=0     进程已退=yes | 文件里有 0   / 写了 10 条 | 全部笔记=0
C2  n=10  wait=900   进程已退=yes | 文件里有 10  / 写了 10 条 | 全部笔记=10
```

`C2` is the control: the same burst, the same abort, 900 ms later, everything
recovered. The harness sees both outcomes, so `C1`'s zero is the app's, not the
harness's.

**Reads re-arm the window.** `flushclock3.js`: one note, then *only* `list_notes`
every ~300 ms for 3 s, then quiet (`ftest/fc3a/`):

```
+218ms WRITE answered
+3218ms last READ (12 reads every ~300ms)
+3843ms quire.db-wal 766352 -> 774592   ← 625 ms after the last READ, not after the write
```

If reads did not touch the queue, the file would have moved at ~+820 ms.

## Cause

The flush is a restartable one-shot timer
(`src/app/controller.rs:3330-3336`), armed by every recorded batch:

```rust
state.install_flush_hook(Box::new(move || {
    …
    t.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(600),
        move || { … s.persistence_force_flush(); … },
```

and `AppState::record` calls that hook unconditionally
(`src/app/state.rs:1881-1886`). So "quiet for 600 ms" is measured from the last
*record*, not from the last thing the user did — and the queue grows without
bound while the deadline keeps sliding.

The reason a read is a record: `arm_pump` logs every call, including reads, and
the log is a settings row. `src/app/mcp_tools.rs:87`:

```rust
s.mcp_log_push(&name, result.is_ok(), &log_message(&args, &result));
```

so `list_notes` → one `Change::SettingSet` → `record` → the timer restarts. The
service itself would not have behaved this way: `PersistenceService::record_all`
anchors the deadline on the *first* batch of a period
(`quire-core` `db0e95f`, `src/services/persistence.rs:183-190`, `if
dirty.since_ms.is_none()`), and `next_deadline_ms()` (`:201-205`) exposes that
anchor — the shell ignores both and re-arms from "now" instead.

Nothing can see the lag. `AppState::persistence_pending()`
(`state.rs:1903-1910`) exists exactly for this question and has one caller
outside its own test (`tests/integration/persistence_test.rs:301,303`): no
footer count, no notice, no log line. The paths that must not read stale data
know the problem and solve it locally by force-flushing first —
`db_refresh` (`state.rs:9956-9961`), `db_markdown_table` (`:13003-13005`), the
sync snapshot (`:15672-15681`), `reclaim_attachments` (`:3342-3346`), page
versions (`:5307`) — which is why the app *looks* correct while the file lags.

Two documents in the tree disagree with the measurement and with each other:
`state.rs:9940` says "every write in this app is debounced by 300 ms
(`PersistenceService`)" — true of the service's own constant
(`DEFAULT_DEBOUNCE_MS`, `persistence.rs:77`) and not true of the app, which is
600 ms; `src/bin/quire_typing.rs:5` names the 600 ms flush. Whatever the number,
it is the Slint timer that decides when bytes move.

## How the previous eight-arm matrix got this wrong

Recorded because the mistake was mine, not the app's, and the fix to it is what
produced the numbers above. `durab.sh` arms A1–A8 reported **0 notes lost in
every arm**, including "crash 150 ms after a 20-note burst". Three separate
reasons:

1. `bulk.js` ends every burst with a `list_notes` *read*, and the harness then
   slept, then killed. That trailing call is itself a record (see above), so the
   arm measured "time since a read the harness performed", not "time since the
   last write".
2. The crash arms ran `oversize-probe.js` through `bash`, so the wait included
   node's start-up (~200–400 ms) on top of `sleep 0.15`. The flush fired during
   that ramp: the abort landed on an empty queue.
3. `stop kill` measures the time the kill is *issued*. `taskkill //F` plus
   `wait_port_free`'s 1 s polling loop gave the app up to a second more of event
   loop than the arm's `wait=` field claims.

The replacement (`durab3.js`) sends the abort from the process that did the
writing, and the file is read from a copy made after the pid is gone. That is the
version that produced `C1`'s zero.

## Candidate fixes

- **Bound the staleness, not just the burst.** Arm the timer to the service's
  own anchor instead of `now + 600 ms`:
  `next_deadline_ms()` minus `now`, clamped to `0..=600`. A quiet period still
  ends 600 ms after the last change, but a burst that never goes quiet writes
  within ~600 ms of its *oldest* unsaved change. This is the service's existing
  model; the shell has to stop overriding it.
- **Do not let a read move the deadline.** Two ways: make the MCP log
  in-memory (a ring buffer published to the settings page, persisted only on a
  real write or on exit), or reuse the predicate that already exists —
  `record()` computes `settings_only` at `state.rs:1875-1877` for the cache
  stamp and could give such a batch a shorter arm (`min(600, remaining)`), since
  a log line is not what the user would miss.
- **Flush when the process is leaving in any way**, not only on the clean exit
  path (`main.rs:1370`). A `Drop` on `AppState` cannot run after an abort, but
  Windows gives a console/WM_CLOSE path a chance: the existing `--quit` handler
  and `flush_pending_edit` already do the right thing; what is missing is a
  short-interval safety tick (e.g. `flush_if_due` on the 250 ms pump that
  already exists, which the service was written for — `persistence.rs:12` says
  the host "call[s] `flush_if_due` (its own timer)" and nothing in this shell
  ever does).
- **Say it in the UI**, in the footer rather than the top bar: `有 N 条改动还没写入`
  from `persistence_pending()` + `pending_len()`. The number is what makes the
  next "the app crashed and I lost the note" report answerable.
- Tests to add: `C1` as an explicit integration test with a fake clock
  (`PersistenceService::new(repo, clock, debounce)` already takes one) — record,
  drop the service without flushing, reopen, assert the row is *not* there; then
  the same with a flush, assert it is. That pins both the window and the fix,
  and it needs no process killing.
