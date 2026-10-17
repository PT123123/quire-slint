# 26 · 恢复点比屏幕上的内容旧十分钟，而核心算出的两个备份事实界面从来不说

**Severity: medium** — the measurement first, because it is the part that is not
an opinion: for the first ten minutes of a session the only thing to fall back to
is the snapshot taken when the session *opened*, and two of the four facts the
storage layer hands the shell about backups are read by nobody.

## Evidence — when does a backup actually appear

`bash snapclock.sh` with `bash snappeek.sh` sampling read-only beside it
(`ftest/m2-S1.timeline.log`, `ftest/m2-S1.peek.log`). One session, `--db
m2-S1/quire.db`, two writes: 2 notes at t+1 s (SA), 2 more at t+636 s (SB),
sampled every ~45–60 s. `main=` is the live library counted from a copy of
`quire.db`+`-wal`+`-shm`; `bakN=<count>@<mtime>` is the row count inside that
generation and the moment it was written.

```
session pid=172176
SA: wrote 2 notes ids=1,2
t=1s    main=2   quire.db.bak1=0@1792197555        [after SA]
t=128s  main=2   quire.db.bak1=0@1792197555        [minute 2, idle]
t=191s  main=2   quire.db.bak1=0@1792197555        [minute 3, idle]
t=254s  main=2   quire.db.bak1=0@1792197555        [minute 4, idle]
t=317s  main=2   quire.db.bak1=0@1792197555        [minute 5, idle]
t=380s  main=2   quire.db.bak1=0@1792197555        [minute 6, idle]
t=443s  main=2   quire.db.bak1=0@1792197555        [minute 7, idle]
t=506s  main=2   quire.db.bak1=0@1792197555        [minute 8, idle]
t=569s  main=2   quire.db.bak1=0@1792197555        [minute 9, idle]
t=612s  main=2   quire.db.bak1=0@1792197555
t=632s  main=2   quire.db.bak1=0@1792197555        [minute 10, idle]
SB: wrote 2 notes ids=3,4
t=636s  main=4   bak1=@1792198192  bak2=@1792197555  [after SB]
t=660s  main=4   quire.db.bak1=4@1792198192  quire.db.bak2=0@1792197555
t=836s  main=4   bak1=@1792198192  bak2=@1792197555  [after clean quit]
```

Six things, each one a line above:

1. the two SA notes were durable from t≈1 s (`main=2` is read out of the file, not
   from the session), and the newest backup for the next **631 seconds** held
   **zero** of them;
2. `bak1`'s mtime is the session's open time and does not move until t=637 s, so
   there is no snapshot timer — the generation changed 1 s after a *write* crossed
   the 10-minute mark (`DEFAULT_SNAPSHOT_INTERVAL_MS = 10 * 60 * 1000`, core
   `services/persistence.rs:84`), which is the flush path calling
   `snapshot_if_due()`;
3. the interval is measured from the *previous* snapshot, whose `last_ms` is set at
   construction (`persistence.rs:132`: `last_ms: now`), i.e. from session open;
4. an idle session never snapshots. SA left `pending = true` at t=1 s
   (`persistence.rs:321`, set in `write()` on a successful apply) and the period
   elapsed at t=600 s — and nothing happened, because `snapshot_if_due()` is only
   reached from `flush_if_due()` / `force_flush()` (`persistence.rs:210-213`, `:235-237`), and the shell's only caller of either is the 600 ms SingleShot that
   a *recorded batch* arms (`src/app/controller.rs:3330-3336`). No batch, no tick,
   no snapshot;
5. after SB, `bak1` = 4 and `bak2` = 0: the good copy of the user's own two rows
   existed for one instant in the main file and never in a backup;
6. the clean `--quit` at t=836 s wrote **no** new snapshot (mtime unchanged) —
   `pending` was false, so the last snapshot stays whatever the last
   >10-minute-crossing write produced.

Put 1 and 6 together for the shape of the exposure: **the recovery point advances
at most once per ten minutes of writing, and never at shutdown.** A user who works
for nine minutes and then hits a bad write, a refused merge, or `21`'s
`{"notes": []}` falls back to the state the library had when they opened the app.

## Evidence — what that looks like to a user, measured

`bash memmode2.sh L2` (`ftest/m2-L2/`): a two-note library written and flushed,
`quire.db` overwritten with 4 KiB of noise, `-wal`/`-shm` deleted, `.bak1` left
untouched, then restarted.

```
L2 seed: 文件里 notes=2
L2 破坏后: 4096 quire.db  212992 quire.db.bak1
L2 console.damaged.log:
quire: the database was unreadable and was restored from
".../m2-L2/quire.db.bak1" — edits made since then are lost
L2 文件里 notes=2 -> 0
L2 文件里的行: []
```

The recovery is real and it is careful — the noise is kept as `quire.db.corrupt`,
the snapshot is validated by opening it (`core/storage/backup.rs:246-255`), the
restored file is immediately re-snapshotted — and it gives back an **empty
library**, because this was the library's first session and the only snapshot in
existence was taken when that session opened, before any note existed. "edits made
since then are lost" is true and unreadable: it does not say when *then* is, how
many rows are on the other side of it, or that the file they were sitting in was
fine until this start.

The banner the user does get is
`src/main.rs:633-637` —

```rust
notices.push(format!(
    "the database was damaged — restored from a backup ({})",
    from.display()
));
```

— English, in an app whose every other `db_notice` is Chinese
(`controller.rs:2385` `还没有模板 — 先把此页面保存为模板。`, `:3353`
`保存失败 — 这次的改动没有写入资料库：{error}`, `:6580` `已清空回收站，{n} 项`).
The sweep (records `01`–`16`) never saw it: no scene triggers a recovery, and a
rendered banner needs a damaged library to exist first.

## Evidence — two facts the core computes and the shell drops

`grep -rn "backup_failed\|take_snapshot_error\|is_clean" src/` over the desktop
crate: **no matches.** So:

- `OpenReport::backup_failed` (`core/storage/backup.rs:62-67`) is documented as
  "Startup continues — the database itself opened — but this session has no
  insurance behind it." `main.rs:594-600` destructures `report.recovered_from` and
  `report.migrated_from` and never looks at it. The only place it surfaces is
  `report.log()`'s third branch (`backup.rs:94-96`), an `eprintln!` — the same
  invisible channel `25` is about.
- `PersistenceService::take_snapshot_error` (`persistence.rs:264-271`) says of
  itself: "this is how the app gets to say *this session is not backed up*". The
  app never asks. A `.bak` that cannot rotate because the folder is full, or a
  `VACUUM INTO` that fails on a locked generation, produces one
  `logging::warn("periodic database snapshot failed: …")` (`persistence.rs:296`)
  and then silence for the rest of the session. Note `snapshot()` is deliberately
  forgiving — `backup.rs:154-160` treats retention as housekeeping and
  `persistence.rs:283-296` stamps `last_ms` *before* running the hook, so a
  persistently failing disk yields exactly one log line per ten minutes and no
  user-visible state change, ever.
- `OpenReport::is_clean()` — the one helper that answers "should I warn the user?"
  in a single call — exists only to be called by tests.

Not measured: a failing snapshot. Producing one needs a read-only or full library
folder, which this rig cannot create without touching the user's `%APPDATA%`. The
claim is the grep plus the code above, and it is stated that way.

## Candidate fixes

- Say how old the backup is. `recovered_from` is a `PathBuf`; `stat` it and put the
  number in the banner: `资料库损坏，已用 {mtime} 的备份恢复 — 之后编辑的 N 个页面不在了`.
  The user can then decide whether to go look for `quire.db.corrupt` before
  continuing, which is the decision the current sentence prevents them from making.
- Translate the two startup banners, and while in there give `quire.db.corrupt` a
  line: it is the file that may still hold their work, and nothing points at it.
- Read the two facts that already exist. `main.rs` can push
  `report.backup_failed` into the same `notices` vector —
  `本次会话没有可用的备份（{e}）` — and the controller's existing 600 ms tick
  (`controller.rs:3350-3360`), which already asks `persistence_last_error()` and
  was the fix for a silent save failure, is the natural place to ask
  `take_snapshot_error()` beside it. Both are one `if let` in code that already
  exists for the other half of the same question.
- Decide on purpose whether shutdown should snapshot. Today it does not unless ten
  minutes have passed since the last one. A `force_flush`-then-unconditional-
  `snapshot()` on the quit path costs one `VACUUM INTO` per session (core's own
  measurement: 23–64 ms for a 2.3 MB workspace, docs/PERFORMANCE.md) and closes the
  "everything since open" window for the *deliberate* exit case; keeping the period
  for mid-session is fine. If the answer is "no", the notice should say the backup is
  the one taken at start, because "restored from a backup" reads like recent.
- The `pending` gate at `persistence.rs:283` means a user who types for nine
  minutes and then stops for one has no snapshot at all. If the interval is meant to
  bound *time*, the shell could arm a slow timer instead of relying on the write
  cadence to call `flush_if_due()`.
