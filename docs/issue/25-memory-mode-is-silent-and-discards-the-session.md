# 25 · 资料库打不开时，会话静默跑在内存里 — 那一行 stderr 在发行版里没有去处

**Severity: high** — the app cannot reach the user's library, tells no one, shows
an empty document where their notes are, accepts every edit as if it had written
it, and drops all of them on exit. Two code paths get here and neither one puts a
word on the window.

## Evidence

`bash memmode2.sh L1` (`ftest/m2-L1/`), three sessions against
`sandbox-20261017/.../quire-slint.exe --db m2-L1/quire.db`:

```
L1 seed: 文件里 notes=2  (wrote 2 notes in 747 ms; the live session sees 2 of them)
L1 锁控自证: LOCK-HELD (holder windows pid 122504)
L1 console.damaged.log:
quire: database unavailable (storage open failed: open database: unable to open
database file: .../m2-L1/quire.db); running in memory
quire: mcp server on 127.0.0.1:5880
L1 该会话看到的库:      session sees: notes=0 tasks=0
L1 内存模式里的 3 次 create: wrote 3 notes in 953 ms; the live session sees 3 of them
L1 会话结束前看到的库:  session sees: notes=3 tasks=0
L1 解锁自证: LOCK-FREED
L1 内存模式会话跑完:    文件里 notes=2 -> 2
L1 文件里的行: [{"id":1,"title":"durab-130116-0"},{"id":2,"title":"durab-130116-1"}]
L1 锁释放后重开一次:    session sees: notes=2 tasks=0
```

Read the six lines in order, because each one is a different promise broken:

- the 2 seeded rows were in the file, so the session that follows had a library
  to show and showed `notes=0` instead — the user's library is **missing from
  view**, not read-only;
- three `create_note` calls answered with ids `1`, `2`, `3`
  (`m2-L1/bulk-marks.json`) and the session then reported seeing 3 notes, so
  every caller — the keyboard, the mouse, an agent over MCP — gets a success
  shape. Those ids collide with rows `1` and `2` that are sitting in the file
  right now;
- after the session, the file still says 2, with the two original titles. The
  three new notes were never written and nothing says so;
- reopening once the lock is released shows `notes=2`: the work is gone, not
  deferred.

`m2-L1/quire.log` for the whole three-session run:

```
2026-10-17T00:32:25.274Z [info] session started (pid 169764)
2026-10-17T00:32:29.397Z [info] session ended
2026-10-17T00:32:37.188Z [info] session started (pid 173800)
2026-10-17T00:32:42.151Z [info] session ended
2026-10-17T00:32:51.180Z [info] session started (pid 170192)
2026-10-17T00:32:52.543Z [info] session ended
```

The middle session is the one that could not open the library, and the log
carries no trace of it. `quire.log` is the artifact this product has for
"what happened last time" — the abort record (`01`-adjacent, `#9`) is read from
it on the next start — and a memory-mode session looks exactly like a healthy
one.

**The one line that does exist cannot be seen.** `src/main.rs:6` is

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

so the shipped binary is a GUI-subsystem process: launched by double-click or
from the 开始菜单 it has no console, and `eprintln!` at `src/main.rs:604` writes
to a stream nobody holds. The debug build I ran here inherits Git Bash's
redirected stderr, which is the *only* reason the line above exists at all —
that is a harness artifact, not a user-visible signal. Not measured on a release
binary: this pass built no release exe (no packaging without approval), so the
sentence "a double-clicked release session prints nothing" is an inference from
`main.rs:6`, labelled as such.

**The control that shows what the app is capable of.** `bash memmode2.sh L2`
corrupts the library instead of locking it (4 KiB of `/dev/urandom` over
`quire.db`, `-wal`/`-shm` deleted, `.bak1` untouched):

```
L2 破坏后: 4096 quire.db / 212992 quire.db.bak1
L2 console.damaged.log:
quire: the database was unreadable and was restored from
".../m2-L2/quire.db.bak1" — edits made since then are lost
L2 文件里 notes=2 -> 0
```

That path has a `report.log()` line *and* a banner (`main.rs:632-638` pushes
`recovered` into `db_notice`). The memory path has neither. The difference is not
capability, it is that `Err(e)` at `main.rs:602-606` only reaches for
`eprintln!` and leaves `repo = None`, while the `Ok` arm calls `report.log()` and
fills `recovered`.

## Cause

`src/main.rs:594-607` — the failure arm:

```rust
Err(e) => {
    eprintln!("quire: database unavailable ({}); running in memory", e);
    None
}
```

`src/main.rs:630-644` — the notice queue is built from exactly two facts, and
neither is "there is no library":

```rust
let mut notices: Vec<String> = Vec::new();
if let Some(from) = &recovered { notices.push(format!("the database was damaged — …")); }
if moved_from.is_some()        { notices.push("the library moved to your user profile".into()); }
```

A second, same-shaped path is `src/app/state.rs:776-786`, inside `AppState::new`:

```rust
let loaded = match repo.take() {
    Some(r) => match r.load() {
        Ok(state) => { repo = Some(r); Some(state) }
        Err(e) => { eprintln!("quire: load failed ({e}); running without persistence"); None }
    },
    None => None,
};
```

Here the file opened fine and the *read* failed; `repo` is left `None`, so
persistence is off (`state.rs:857-859` maps over `repo`) and the library on disk
is invisible — again only on stderr, again with no `db_notice`. That path was not
driven; it is the same shape and reachable from a partially-written file.

And when the user does go looking, two settings actions reassure them.
`src/app/state.rs:3310-3317`:

```rust
pub fn backup_now(&self) -> Result<String, String> {
    match self.repo.as_ref().filter(|r| r.path().is_some()) {
        …
        None => Ok("运行于内存模式 — 无需备份".into()),
    }
}
```

`Ok`, with a message that says there is nothing to protect. `reclaim_attachments`
(`:3334-3337`) answers the same way. Both are reached only from the settings
dialog (`controller.rs:3401`, `:3413`), so 运行于内存模式 is a string the user can
*only* meet by pressing 立即备份 after their library has already gone missing —
and it arrives as a success notice, not a warning. (Source-read: the dialog is
not drivable from this rig, per the no-UI-automation rule.)

**The right sentence already exists and is structurally unreachable.** The shell
already says 保存失败 — 这次的改动没有写入资料库 (`controller.rs:3353-3357`) on
the same 600 ms timer the flush hook arms, and the arm is unconditional
(`state.rs:1881-1886`: `record()` calls the hook whether or not there is a
service to record into). In memory mode the timer therefore fires, calls
`persistence_force_flush()` (no-op, `state.rs:1895-1901`), then asks

```rust
pub fn persistence_last_error(&self) -> Option<String> {
    self.persistence.as_ref().and_then(|p| p.take_last_error()).map(…)
}
```

(`state.rs:1920-1925`) — and `self.persistence` is `None`, so the answer is
`None` and nothing is set. The session checks "did the save fail?" every 600 ms
for its whole life and can only ever conclude "there was no save to fail".
`persistence_pending()` (`state.rs:1903-1910`) has the same shape and the same
`None`; it still has no caller outside
`tests/integration/persistence_test.rs:301,303` (which `20` also notes).

## Candidate fixes

- Make the arm put a fact in the queue. `main.rs:602-606` can set a third local,
  e.g. `memory_only = Some(e)`, and `:630-644` push
  `因变量：无法打开资料库（{e}）— 本次会话的内容不会保存`. The notice mechanism
  already exists, is already a queue, and already survives the startup ordering;
  this is a three-line change in the arm that has the error in hand.
- Use the state the UI can hold. `db_notice` is one line and disappears; the
  condition lasts the whole session, so a persistent affordance is the honest
  shape — the same place that renders 保存失败 (`controller.rs:3348-3357`) can
  carry a 未连接资料库 marker, and every save path already knows
  `self.persistence.is_none()`.
- Refuse, or make it loud enough to refuse. A session that cannot write is not a
  degraded editor, it is a trap: everything looks like it worked. Consider
  requiring a flag for memory mode (`--no-db`) and exiting with a message when
  the default library cannot be opened, with the *existing* retry-once-after-a-
  second behaviour for the transient case (a file briefly held by a backup or an
  indexer is what actually causes this in the field).
- `backup_now`'s memory branch should be `Err`, not `Ok`. "无需备份" is true of
  the disk and false of the user's intent. `Err("没有可备份的资料库 — 本次会话未连接文件")`
  renders through the same notice path (`controller.rs:3403-3409`) and reads like
  the problem it is. Same for `reclaim_attachments`.
- Whatever happens to `state.rs:784`, `load failed` deserves the same treatment —
  the two arms should not differ in loudness, and today neither of them differs.
- Add the log line. `logging::warn` at the `Err` arm would put
  `database unavailable` into `quire.log`, which is the artifact the *next*
  session already reads and shows. That alone makes a support question answerable.

## How the first version of this harness got it wrong

Recorded so the next pass does not re-derive it. `memmode.sh L1` reported
`notes=1 → notes=4`, i.e. memory mode appeared to write through — three harness
faults, none of them the app's:

1. the lock path was built with `sed 's|/|\\|g'` over an MSYS path, giving
   `C:\c\Users\…`; PowerShell threw `DirectoryNotFound` into `lock.out` and the
   file was never held, so the session opened the library normally;
2. `start()` treated an existing `mcp.json` as proof the listener was up, and
   the *seed* session's `mcp.json` is still in the same directory. The burst then
   hit `connect ECONNREFUSED 127.0.0.1:5880` and the three "lost" rows were
   actually the seed's own rows;
3. `rows_in_file` copied `quire.db` while the exclusive handle was still open, so
   the copy failed, the snapshot contained only `-wal`/`-shm`, and the count came
   back empty. Worse, that made the reopen control read memory mode too — a
   second `session sees: notes=0` that looked like the bug persisting after the
   lock was gone.

`memmode2.sh` fixes all three and prints its own gates: a `LOCK-HELD` probe that
must pass before the arm runs (`exit 6` if the lock is not real — an arm that
measures nothing must not produce a record), a `waitport.js` TCP probe instead of
the pointer file, a `LOCK-FREED` probe after releasing, and a reopen leg whose
`notes=2` is what proves the file was readable again. `bash memmode2.sh L1` is
self-refusing by construction; the earlier green-looking `4` was the harness
talking to itself.
