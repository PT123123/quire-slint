# 14 · a second launch blocks on a modal message box

**Severity: high** — the rejected launch is a live process that never returns
until a person clicks OK, and no flag makes it non-interactive.

## Symptom

```
quire-slint.exe --db ./si4.db --auto-exit 6 &      # 22:52:47 session started (pid 161972)
sleep 2
timeout 30 quire-slint.exe --db ./si4.db           # 06:52:49 → killed at 06:53:19, rc=124
```

Nothing on stdout or stderr from the second launch, and no second
`session started` line in `quire.log`. The first session logged `session ended` at
22:52:53 — **the second launch stayed blocked for 26 more seconds after the session
it collided with had already exited.** An earlier instance of the same test left
the process alive **11 minutes** (`tasklist` showed pid 172928, 38,264 K, from
22:45 still running at 22:56) until killed by PID.

## Cause

The claim runs before anything else (`src/main.rs:451-540`, ADR-0136 — "the very
first thing a session does, before logging, the database or the window"). Same
version already running ⇒ `SecondLaunch::AlreadyRunning` (`src/platform/quit.rs:142-143`),
and the `announce` callback the caller passes ends in
`quire::platform::notify(title, &body)` (`main.rs:537`), which is
(`src/platform/mod.rs:98-124`):

```rust
unsafe { let _ = MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION); }
```

A synchronous, modal `MessageBoxW` on the launch thread, with **no timeout** and no
parent window. The comment notes only the other case:

> A message box that cannot be shown (a session 0 service, a headless verify run)
> returns 0 rather than blocking

— which is true when there is no desktop to draw on. On a normal interactive
session the box *can* be shown, and then the call does not return until someone
dismisses it.

Two consequences that are not about the dialog being annoying:

1. **`--auto-exit` cannot save the launch.** `parse_launch_args()` is at
   `main.rs:553`, *after* the claim, so no CLI flag — `--auto-exit`,
   `--measure-startup`, `--dump-state` — has been read when the block happens. The
   only thing that skips the claim is `--quick-note` (`main.rs:475`, "a harness run
   rather than a session"), and that also skips the window.
2. **A script that launches the app while the user's Quire is open hangs.** Any
   consumer of `bench_matrix.ps1` / `startup_bench.ps1` on a developer machine with
   a session running stops there — with no output to say why, and, as observed, the
   hung process survives the session it was waiting on.

## Positive control (this is not the quit channel being broken)

```
quire-slint.exe --db ./si7.db --auto-exit 25 &     # 22:58:20 session started (pid 148996)
quire-slint.exe --db ./si7.db --quit
  rc=0, stdout: "quire: the running instance accepted the quit request"
  # 22:58:25 session ended  → 5.1 s, clean
```

`--quit` (`main.rs:284-300`) writes to the same pipe and returns immediately. A
first attempt at this test appeared to show `--quit` failing; that was my own
force-kill of the live session in the previous experiment leaving the pipe's owner
dead, and the result is retracted: with a session that exits on its own, the quit
channel works exactly as ADR-0105 describes.

## The `.md` door, measured a second time (added later in the same pass)

The arm above collided with a plain `--db` launch. `secondwin.sh` runs the collision a
double-clicked file actually walks through — `--open <file>` — against a session that
was started with `--auto-exit abc` and therefore never exits (record `36`'s first
finding), and reads the refused launch's windows with `winlist.ps1`, an `EnumWindows`
walk (`GetWindowThreadProcessId`, `IsWindowVisible`, `GetWindowText`, `GetClassName`,
`GetWindowLongPtrW`). Read-only: nothing was clicked, typed into, or photographed.

```
session 1: pid=170296 at +2.1s           (--auto-exit abc → the session lingers)
second launch: `--db <case>/quire.db --open <fx>\good.md`, windows pid=165656 at +4.0s
── +10.6s, +31.1s, +60.8s: the three inventories are identical ──
  alive=alive   Get-Process: 16978102 | Responding=True | title=Quire 已在运行
  EnumWindows:  hwnd=0x10310B6 visible=True class=#32770 title=Quire 已在运行 exstyle=0x10101
                hwnd=0x860C6E visible=False class=MSCTFIME UI
                hwnd=0x1330DA6 visible=False class=IME
                ws=38MB  threads=6 (5 by the last sample)
  console bytes=0        ('session started' lines in the log: 1, i.e. session 1's)
  second db exists=no    (nothing created at the --db path it was given)
── then --quit ──
  rc=0 out=quire: the running instance accepted the quit request
  +5s after --quit: second=alive   first=exited
```

Four things that this adds to the shape described above:

- **The box is the only window the process has, and it is a real dialog.** `class=#32770`
  is the Win32 dialog class `MessageBoxW` creates, and the caption is the string at
  `main.rs:504`. `exstyle=0x10101` is
  `WS_EX_DLGMODALFRAME|WS_EX_WINDOWEDGE|WS_EX_APPWINDOW` — it has a taskbar button, so
  this is a window a person must find and dismiss, not an invisible wait.
- **`Responding=True`.** The refused launch is not hung in the Windows sense; its thread
  is sitting in a modal message loop that is perfectly healthy. Nothing will ever
  time this out from outside.
- **The launch never touches the library it was told to use.** No file exists at its
  `--db` path, and its console holds zero bytes — the "never opens the database and never
  draws a window" comment at `main.rs:542-544` is confirmed for the file-association door
  too, including the half that matters: the `.md` the user double-clicked is not opened,
  not queued and not forwarded to the live session. There is no such thing as
  "double-click a file while Quire is open" — the file is dropped on the floor behind a
  dialog.
- **`--quit` does not clear it.** The quit retired session 1 (`first=exited`) while the
  refused launch stayed alive, which is the same asymmetry the 11-minute observation at
  the top of this record describes. A second instance of that survived long enough to be
  caught: an earlier run of the rig cleaned its launches up with `taskkill //PID $B`
  where `$B` was `$!` — a bash job number, not a Windows pid — so the kill silently did
  nothing and the refused launch outlived its own arm. It was found by querying
  `Win32_Process` for the sandbox exe (pid 143876, started 11:24:43, argv
  `… --db …/cliprobe2/second/quire.db --open …/good.md`, bash job 1266), and at ~2.5 min
  uptime it measured `MainWindowHandle=23005654`, `Responding=True`,
  title `Quire 已在运行`, 3 threads, 37 MB — a normal healthy process, still on the
  desktop, two minutes past the session it had collided with. Only
  `taskkill //PID 143876 //F`, using the pid read back from the process table, ended it.
  That harness bug is also the reason the rig now reads the Windows pid out of
  `quire.log` or `Win32_Process` and never trusts `$!`.

## Candidate fixes

- Bound the notice: `MB_OK | MB_SETFOREGROUND` plus a thread with a timeout, or
  `MessageBoxTimeoutW` / a `TaskDialog` with `TDF_CALLBACK`-driven auto-dismiss,
  so an unattended launch stops in ~5 s and the process exits instead of sitting on
  a desktop.
- Or split the two audiences, which is the smaller change: a launch that carries
  **any** harness flag (`--auto-exit`, `--db` on a non-default file,
  `--measure-startup`, `--dump-state`) announces on stderr and exits; only a
  double-click (argv empty) gets the modal. `--quick-note` already establishes that
  the claim may be skipped by a harness; this generalises it without weakening the
  rule for the user who double-clicked twice.
- Set an exit code that says what happened. Today the launch returns `Ok(())` after
  the dialog closes (`main.rs:541-547`), so a caller cannot distinguish "I became
  the session" from "I was refused" — the `-Force`/retry logic a deploy wants has to
  parse the log.
- Whichever shape: give `bench_matrix.ps1`/`startup_bench.ps1` a pre-flight check
  that fails fast with "a Quire session is already running" instead of hanging on a
  dialog the harness will never see.
