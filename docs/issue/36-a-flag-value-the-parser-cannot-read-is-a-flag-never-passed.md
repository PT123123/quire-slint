# 36 · a flag value the parser cannot read is a flag that was never passed

**Severity: medium** — `--auto-exit` is the only thing that bounds a session, and its
parse is `v.parse().unwrap_or(0.0)` (`src/main.rs:97`) feeding
`if args.auto_exit_secs > 0.0` (`src/main.rs:1212`), so `--auto-exit abc`,
`--auto-exit -5`, `--auto-exit 0`, `--auto-exit=8` and a `--auto-exit` that is the last
word in the command line all mean **run forever** and say nothing. Measured: the control
exits 9.98 s after launch; each of those five is still alive 20 s later (90 s for `abc`
in a second rig) and has to be stopped by PID. The same shape is on the other two doors:
`--share-port 99999` means *no share at all*, `--share-port notanumber` against a
library that has `lan.share=1` means *open `0.0.0.0:5877` anyway*, and there is no
`--no-mcp` — the string falls into `_ => {}` (`src/main.rs:168`) so 5880 stays up
behind a token the caller thought it had turned off. A second `--open` silently replaces
the first, so `--open good.md --open typo.md` imports **nothing** and reports only the
typo, and `--share 5891` — the spelling `main.rs:46` advertises — opens the share on
5877 and tries to import a file called `5891`. Every one of these arms exits 0, prints
no complaint, and writes no settings row.

## Evidence

`flagprobe.sh` and its three-arm follow-up `flagprobe4.sh` — fifteen arms, each its own
throwaway `--db <case>/quire.db`, `--dump-state` so the moment the event loop exists is
on the record, the listeners of **that session's own pid** read out of `netstat` (both
rigs abort if 5880 is held by anyone else), and the answer read back from a snapshot
copy of the file. `flagprobe2.log` and `flagprobe4.log` are those batches; the two share
arms that bind (`5891`, `0`) came from the first batch, `cliprobe-flags.log`.

| arm | the part of argv that is the question | loop at | listeners | what the session did |
|---|---|---|---|---|
| `ctl` | `--auto-exit 8` | 4.78 s | `0.0.0.0:5878 127.0.0.1:5880` | **exited at 9.98 s** |
| `nosuch` | `+ --nonsense-flag` | 3.57 s | same | exited at 10.08 s, console byte-for-byte the control's |
| `nomcp` | `+ --no-mcp` | 3.71 s | `0.0.0.0:5878 `**`127.0.0.1:5880`** | MCP up, `mcp.json` written, exited 9.84 s |
| `open2` | `--open good.md --open nope.md` | 3.95 s | same | `import failed: cannot read …\nope.md`, `pages=19` |
| `exitbare` | `--auto-exit` (last word) | 3.55 s | same | **alive 20 s later**, stopped by `--quit` rc=0 |
| `exittypo` | `--auto-exit abc` | 3.55 s | same | **alive 20 s later** (and at 5/10/20/40/60/90 s in `cliprobe2.sh`) |
| `exitneg` | `--auto-exit -5` | 3.62 s | same | **alive 20 s later**, `--quit` rc=0 |
| `exitzero` | `--auto-exit 0` | 3.50 s | same | **alive 40 s later** |
| `eqform` | `--auto-exit=8` | 3.69 s | same | **alive 20 s later** — the `=` form is not a flag at all |
| `sharehi` | `--share-port 99999` | 3.58 s | `0.0.0.0:5878 127.0.0.1:5880` | **no share listener, no line about sharing** |
| `shareneg` | `--share-port -1` | 3.56 s | same | same |
| `shareempty` | `--share-port ""` | 4.29 s | same | same |
| `sharenum` | `--share 5891` (space, not `=`) | 3.71 s | **`0.0.0.0:5877`** `0.0.0.0:5878 127.0.0.1:5880` | `quire: import failed: cannot read 5891` — the port became a file name |
| `pos2` | `good.md nope.md` as positionals | 3.77 s | same | `pages=20` — the first imported, the second dropped with no word |
| `dbnum` | `--blocks 4000000000` | **never** | `[]` | `memory allocation of 1440000000000 bytes failed`, gone at 6.26 s; library left with all 26 tables, `pages=0 blocks=0 settings=0`, `session started` and **no `session ended`** |

Two arms do what was asked, which is the control that the flags themselves work:

```
[shareok]   --share-port 5891   listeners=[0.0.0.0:5878 0.0.0.0:5891 127.0.0.1:5880 ]
            console=quire: LAN sharing on 0.0.0.0:5891|quire: mcp server on 127.0.0.1:5880|…
[sharezero] --share-port 0      listeners=[0.0.0.0:5878 0.0.0.0:54932 127.0.0.1:5880 ]
            console=quire: LAN sharing on 0.0.0.0:54932|…
```

`0` is a *valid* `u16` for `parse()`, so `--share-port 0` is not a typo — it asks for
"some port" and gets an ephemeral one, and the app reads the bound address back and
prints it, which is the right thing to do. `99999`, `-1` and `""` are not valid `u16`s,
and for those the flag might as well not have been typed.

`--share 5891` is the one that deserves its own paragraph, because the source's own
comment advertises that spelling (`main.rs:46`: *"Serve the workspace read-only on the
LAN (--share [port], default 5877)"*). The parser has no such arm: `("--share", _)`
takes no value (`main.rs:144-146`), so `--share` binds `DEFAULT_PORT` and the next word
falls to the positional arm (`main.rs:163-167`) and becomes the file to import. The
measured console for that arm is:

```
quire: import failed: cannot read 5891|quire: LAN sharing on 0.0.0.0:5877|quire: mcp server on 127.0.0.1:5880|…
```

The port the user typed was tried as a path, failed, and the share opened on **5877** —
the whole-document-tree, no-credential endpoint of `28` — anyway.

**A discarded port does not close the share — it reverts it.** `flagprobe.sh`'s
`revert` block seeds the row the settings switch writes (`lan.share=1`) into a copy of
a real library and then asks for a port that cannot be read:

```
seeded: [{"key":"lan.share","value":"1"}]
[revert]    --share-port notanumber   listeners=[0.0.0.0:5877 0.0.0.0:5878 127.0.0.1:5880 ]
            console=quire: LAN sharing on 0.0.0.0:5877|…       exited 9.86 s
[retnothing] (same library, no flag at all)
            listeners=[0.0.0.0:5877 0.0.0.0:5878 127.0.0.1:5880 ]
            console=quire: LAN sharing on 0.0.0.0:5877|…       exited 9.59 s
```

So the typo'd arm and the flagless arm are the same session: the request is dropped on
the floor and the persisted default is served. A person who types `--share-port 5891`
wrong, or a script that interpolates an empty variable into that slot
(`shareempty`), gets the library **on `0.0.0.0:5877`** — the whole-document-tree,
no-credential endpoint of `28` — with one line in a console the release build does not
have (`src/main.rs:6`).

## The off switch that does not exist

MCP is on unless `mcp.enable` has been explicitly turned off
(`main.rs:1085`: `launch.mcp || state.setting_flag_or("mcp.enable", true)`), and
`--mcp` is the only CLI word for it. Two arms, the **same library** in both:

```
[mcpseed]  fresh library, no flag      listeners=[0.0.0.0:5878 127.0.0.1:5880 ]  mcp_json=written
           (the settings row the switch writes, inserted by hand into that library:)
           seeded: [{"key":"mcp.enable","value":"0"}]

[mcpoff]   that library, no flag       listeners=[0.0.0.0:5878 ]   mcp_json=no   5880 owner=(none)
[mcpflag]  that library, --mcp        listeners=[0.0.0.0:5878 127.0.0.1:5880 ]   5880 owner=126484 (the session pid)
           settings after the run: mcp.token=… ; mcp.log=… ; mcp.enable=0
```

`--mcp` overrides a deliberate "off" — which is defensible, a flag is a stronger
statement than a row — but there is **no flag in the other direction**: `--no-mcp` is
not a recognised word (`nomcp` above: 5880 up, pointer file written, token minted), so
a user cannot close the agent endpoint for one launch the way they can open it, and the
only off switch is a settings row that has to be written through the UI dialog.
`--no-mcp` is not merely absent: because of `_ => {}` it is *swallowed*, so the launch
looks like it honoured the request — the `nomcp` arm's listeners are the control's,
`0.0.0.0:5878 127.0.0.1:5880`, with a `mcp.json` in the launch directory and a freshly
minted token in the file. The same swallow covers the `=` spelling of any flag: the
`eqform` arm (`--auto-exit=8`) is one of the never-exit arms above.

The flag also does not persist: `mcp.enable` is still `0` after the `--mcp` session
exits, so the next launch is off again. For a door that hands a bearer-tokened endpoint
to any local process, "on for this launch, invisible in the file afterwards" is the
worst of both — the pointer file `mcp.json` is written into the launch's cwd, so the
token and the port land on disk in a place the user was not looking.

## Nothing asked on the command line is remembered

The `settings` read-back after every arm that got one holds exactly the rows the app
writes for itself:

```
mcp.token=… ; mcp.log=[{"at":"2026-10-17 04:0x","tool":"server",…
```

plus `lan.share=1` only where the harness inserted it. No flag measured here —
`--share-port`, `--auto-exit`, `--mcp`, `--no-mcp`, `--nonsense-flag`, `--share 5891`,
`--dump-state`, `--open`, a positional path — writes a row, which is the clean half of
this record: **the CLI cannot leave the library in a more open state than the settings
say** (the `28` finding, holding here too). What the CLI *can* do is fail silently, and
the file will not show that it was ever asked.

## Cause

`parse_launch_args()` (`src/main.rs:67-170`) has eight arms that read a value and
coerce a failure into a default instead of reporting it:

```rust
("--blocks",      Some(v)) => { a.blocks = v.parse().unwrap_or(0); i += 1; }          // :92-94
("--auto-exit",   Some(v)) => { a.auto_exit_secs = v.parse().unwrap_or(0.0); i += 1; } // :96-99
("--page-switch", Some(v)) => { a.bench_pages = v.parse().unwrap_or(0); i += 1; }      // :100-102
("--pictures",   Some(v)) => { a.pictures = v.parse().unwrap_or(0); i += 1; }        // :104-106
("--marks",      Some(v)) => { a.marks = v.parse().unwrap_or(0); i += 1; }           // :108-110
("--code",       Some(v)) => { a.code = v.parse().unwrap_or(0); i += 1; }            // :112-114
("--scroll-step", Some(v)) => { a.scroll_step = v.parse().unwrap_or(8.0f32).max(1.0); }// :119-121
("--share-port",  Some(v)) => { if let Ok(p) = v.parse() { a.share = Some(p); } i += 1;}// :150-154
```

and five shapes of silence on top of that:

- `0.0` is both "no timer" and the *initial value* of the field (`main.rs:71`), so a
  value that will not parse is indistinguishable from the default, and the reader at
  `main.rs:1212` (`if args.auto_exit_secs > 0.0`) treats a legal negative parse
  (`-5` → `Ok(-5.0)`) the same way.
- A value-taking flag as the **last word** matches nothing: the arms are written as
  `("--auto-exit", Some(v))`, so `("…", None)` falls through to `_ => {}` (`:168`) and
  the flag itself is discarded — `--auto-exit` with no argument is not an error, it is
  "no `--auto-exit`".
- The same `_ => {}` swallows every unknown word, so `--no-mcp`, `--auto-exit=8`, and a
  mistyped `--share-prots 5891` are all exactly as loud as not typing them.
- `--share-port` reads `launch.share`, and the share decision is
  `launch.share.or_else(|| if state.setting_flag("lan.share") { Some(DEFAULT_PORT) } …)`
  (`main.rs:1050-1056`) — dropping the CLI value falls through to the persisted default
  rather than to "off". And because `("--share", _)` takes no value, a port after
  `--share` is not a port any more (`main.rs:144-146` + `:163-167`), whatever the field's
  own doc comment at `main.rs:46` says.
- `--open` assigns unconditionally (`main.rs:140-143`), so the last one wins;
  the positional arm only fills an empty slot (`main.rs:163-167`), so extras vanish.
- `mcp.port` is read at `main.rs:1087` with `setting_str("mcp.port")`, and is written by
  nothing in `src/` or `ui/` (`grep -rn "mcp.port" src ui` → 13 lines, all in `main.rs`,
  all reads). It is a door with a hinge on one side only.

`--blocks 4000000000` is the same no-questioning shape with a bigger end: the value
parses (`:93` is `v.parse().unwrap_or(0)` into a `usize`), so nothing complains, and the
bench path asks for 1,440,000,000,000 bytes. The arm's console is two lines —
`memory allocation of 1440000000000 bytes failed` and the `RUST_BACKTRACE` note — no
listener ever opened, and the process is gone 6.26 s after launch. What it leaves is the
part worth reading: the library it had already opened holds all 26 tables with
`pages=0 blocks=0 settings=0` (schema created, seeding never reached), `quire.log` holds
`session started (pid 163100)` and no `session ended`, and a `quire.db.bak1` was written
before the death. That is `22`'s corpse shape produced from a local command line instead
of a request header — and the record's point is the missing step in between, which is a
sanity check on a number a person typed.

## Why a typo here is not only a typo

The whole point of `--auto-exit` is that a harness run cannot outlive its welcome, and
`docs/AGENT_BRIEF_*.md` tells every agent to use
`--db .scratch/<dir>/quire.db --auto-exit <secs>`. When the seconds come from a
variable that is empty or non-numeric, the session does not stop. Then, per `14`, the
**next** launch in the same script — the one that will actually be asked to do the work
— collides with it and stops at a modal `MessageBoxW` with no timeout. `secondwin.sh`
ran that pair end to end: session 1 launched with `--auto-exit abc`, then a second
launch carrying `--open <file>` (the shape a double-clicked `.md` takes, `install/quire.iss:63`).

```
session 1: pid=170296 at + 2.1s            (--auto-exit abc: never exits)
second launch: windows pid=165656, at + 4.0s
── sample at +10.6s / +31.1s / +60.8s — the three inventories are identical ──
  alive=alive  Get-Process: 16978102|Responding=True|title=Quire 已在运行
  EnumWindows: hwnd=0x10310B6 visible=True class=#32770 title=Quire 已在运行 exstyle=0x10101
               hwnd=0x860C6E visible=False class=MSCTFIME UI / hwnd=0x1330DA6 visible=False class=IME
               ws=38MB threads=6 (5 by the third sample)
  console bytes=0
── after --quit ──
  --quit while the box is open: rc=0 out=quire: the running instance accepted the quit request
  +5s after --quit: second=alive  first=exited
  second db exists=no            (nothing at the --db path it was handed)
  cleanup: taskkill //PID 165656 //F
```

One visible window, Win32 dialog class `#32770`, caption 「Quire 已在运行」 (the title
`main.rs:504` hands to `notify()` at `main.rs:537`), an extended style of
`WS_EX_DLGMODALFRAME|WS_EX_WINDOWEDGE|WS_EX_APPWINDOW`, sixty seconds of that, no byte
ever written to stdout or stderr, no library opened, and it outlives the session it was
refused by. `30` reported the same chain from the other end (`--pull` holds the launch
open so `--auto-exit` never fires); this is the *flag* end of it, with no network
anywhere in it.

## Candidate fixes

- **Make a value that cannot be read an error, not a default.** `parse_launch_args()`
  returns `LaunchArgs` with no failure channel (`fn parse_launch_args() -> LaunchArgs`,
  `main.rs:67`, called at `main.rs:553`). Change it to `Result<LaunchArgs, String>` and
  print `quire: --auto-exit needs a number of seconds, got "abc"` and exit 2. That is
  still *after* the single-instance claim (`main.rs:497-545`), so it cannot save a launch
  that collides with a running session — that half is `14`'s — but it does break the
  chain in §"Why a typo here is not only a typo" at its first link, because the session
  that would have lingered forever never starts. It costs no startup time and reaches
  stderr before the database is opened or a window is drawn.
- **If the whole `Result` is too big a step for now, do the two cheap ones**: keep the
  `unwrap_or` but print the fallback (`eprintln!("quire: --auto-exit {v} is not a
  number; running without a timer")`), and change the value arms to also match
  `("--auto-exit", None)` and report "missing value" instead of falling to `_ => {}`.
- **Treat a non-positive timeout as an error.** `0` is already "no timer" by the
  `> 0.0` test; `-5` and `abc` should not be able to hide behind that sentinel.
- **Give `--share-port` a real failure.** If `launch.share` was *asked for* and the
  port did not parse, do not fall through to the persisted default: either exit 2 with
  "not a port", or (smaller) bind nothing and print
  `quire: --share-port "notanumber" ignored; lan.share still serves 0.0.0.0:5877`.
  That second line is the one that turns a silent widening of `28` into a visible one.
- **Add `--no-mcp`, and make the pair symmetric.** `launch.mcp` is a `bool`
  (`main.rs:51`, `:148`); `--no-mcp` should force the server down for the launch, and
  `--mcp`/`--no-mcp` together should be an error. `19`, `21`, `22` and `23` all argue
  that the agent endpoint deserves an explicit, per-launch off switch that does not
  require opening a dialog.
- **`--flag=value` should mean `--flag value`.** Split on `=` before the `match`; every
  CLI in the ecosystem accepts it, and today it is a silent no-op.
- **Make `--share` take its port, or make the comment stop advertising one.**
  `main.rs:46` says `--share [port]`; the arm at `main.rs:144-146` takes nothing, so the
  number lands in the positional arm and becomes a path to import. Either give `--share`
  the optional-value arm the comment promises, or change the comment — but then also
  reject a bare number in the positional arm, because `5891` is not a file the user meant
  to open, and the failure message ("cannot read 5891") is the only thing that says so.
- **Cap the bench knobs.** `--blocks`, `--pictures`, `--marks`, `--code` and
  `--page-switch` (`:93`, `:105`, `:109`, `:113`, `:101`) all turn a typed number into a
  count of things to build, and a `usize` that parses is not a size the machine can
  honour — `--blocks 4000000000` is measured above. Only that one arm was run; the other
  four share the `parse().unwrap_or(0)` shape, and nothing in the parser bounds the
  value. `22` asks for the same protection on the network side;
  `1,440,000,000,000 bytes` is what an unguarded count costs when it comes from a
  keyboard instead of a socket.
- **Say what was dropped.** One rule for the whole parser: an argument that was not
  consumed by an arm prints one line. `_ => {}` (`:168`) is where three of the findings
  above are hiding — `--no-mcp`, `--auto-exit=8`, and any word the parser has never
  heard, which is what a mistyped flag turns into.
- Whichever shape: **pin it with a gate.** `flagprobe.sh` and `flagprobe4.sh`'s fifteen
  arms already assert the shape from the outside (loop reached, listeners of the
  session's own pid, exit observed, snapshot read back); the fix version is "the five
  never-exit arms exit, the three bad-port arms print a complaint and bind nothing,
  `--share 5891` binds 5891 rather than 5877, `--no-mcp` leaves 5880 closed,
  `--blocks 4000000000` is refused". That is a five-minute gate that cannot be written
  today because there is no non-interactive way to ask the parser a question and get an
  answer.

## Not measured, said plainly

- **No pixel claim.** `winlist.ps1` enumerates top-level windows with `EnumWindows`
  (read-only: `GetWindowThreadProcessId`, `IsWindowVisible`, `GetWindowText`,
  `GetClassName`, `GetWindowLongPtrW`) and prints class, title, visibility and extended
  style. It proves a window exists with the modal-dialog class `#32770` and title
  「Quire 已在运行」 (`exstyle=0x10101` = `WS_EX_DLGMODALFRAME|WS_EX_WINDOWEDGE|WS_EX_APPWINDOW`);
  it does not photograph it, and nothing in this pass clicked it or typed into it.
- **`--auto-exit 0` is probably intended** as the "no timer" sentinel; the record's
  complaint about `0` is that it is also the value a typo lands on, not that the
  sentinel is wrong.
- **Whether the modal box's OK button would return** is `14`'s finding (a person must
  click); not re-tested here — dismissing it needed a click, which this pass does not
  do, so the arms end with `taskkill //PID <pid> //F` on a process this harness started.
- **`--share-port 5891` and `--share-port 0` have no settings read-back** (they ran in
  the first batch, whose `run()` predated that line), so "a working `--share-port`
  writes no row" is not claimed — the twelve arms of the second batch are what the
  persistence paragraph rests on.
- **`--share-prots 5891` (a mistyped flag name) was not given its own arm.** It is the
  same `_ => {}` as `--nonsense-flag`, which was, and `--share 5891` covers the port
  going the wrong way.
- **Where `--blocks` asks for its 1.44 TB was not located in the code.** The arm shows
  the request and the abort; the allocation site was not read, and no claim is made that
  it is a `Vec::with_capacity` specifically — `22` documents that shape for the HTTP
  path, this one is inferred to be the bench block builder because `--blocks` is the only
  thing the argv changed.
- **No release binary.** Every arm here ran the sandbox debug exe, which has a console.
  The lines the record calls "invisible in a release build" (`lan bind failed`,
  `import failed`, `mcp server on …`) are `eprintln!` calls in a
  `windows_subsystem = "windows"` binary (`src/main.rs:6`), which is the same inference
  `25` and `28` flag; the debug arms prove the strings are produced, not that a shipped
  user can see them.

## How this harness got it wrong

Three mistakes in this rig, and one in the window probe that goes with it. All four are
worth recording because in each case the fix is the reason the numbers above mean
something.

1. **The exit was never measured at all.** `run()` pulled the requested timeout out of
   `printf '%s\n' "$@" | sed -n 's/--auto-exit \([^ ]*\).*/\1/p'` — but `printf '%s\n'
   "$@"` puts **each argument on its own line**, so `--auto-exit` and `8` never
   appeared on one line, the pattern never matched, and the branch that waits for the
   exit did not run. The only thing left was the cleanup loop, which counts iterations
   until the child is gone; for the control that was 3 iterations, and the line printed
   `still alive 3s after quit, taskkill` for a session that had exited cleanly by
   itself and had never been sent a `--quit`. The first batch's log reads that way on
   all twelve arms. Fixed by walking `"$@"` for the word after `--auto-exit` and
   re-running the batch — which is where `exited_at=9.98` versus
   `exited_within_20s=NO` comes from, and why the second batch says `--quit rc=0`
   explicitly for the never-exiting arms instead of implying it.
2. **The control arm opened a different library than the arm it controlled.** The
   revert block copied the seeded library into `$OUT/revert-nothing` and then called
   `run retnothing`, whose case directory is `$OUT/retnothing` — a name that did not
   exist, so `run()` built a **brand-new** library there. The printed control ("no
   flag, no 5877") was therefore measuring a fresh library, not the seeded one, and it
   contradicted the arm above it. Caught because the two arms reported *different*
   `mcp.token` values from libraries that were supposed to be copies; fixed by matching
   the directory name, and the corrected control is in the table above (`5877` bound
   with no flag at all, same token as the seeded library).
3. **`q.js` with a relative `cygpath -w`** — `cygpath -w` does not absolutise, so the
   one-off verification query after the batch failed with `unable to open database
   file` on a file that plainly existed. Not a product fact; recorded so the next pass
   reaches for `cygpath -wa`.
4. **The window probe's sample labels were wrong by a factor of the whole wait.**
   `secondwin.sh`'s first run polled with
   `while [ "$(el "$T0" "$(now)")" -lt "$t" ]`, and `el` prints `%6.1f` — a
   space-padded float in an integer `[ ]` test, which bash rejects
   (`[: 4.5: integer expected`), so the loop fell straight through and the two samples
   I read as "+10 s" and "+30 s" were taken at +4.5 s and +11 s. The fix is an `awk`
   comparison and, more usefully, printing the measured elapsed time beside the one that
   was asked for — which is why §"Why a typo here is not only a typo" quotes `+10.6 s /
   +31.1 s / +60.8 s`. A duration claim whose sampler cannot compare durations is not a
   measurement.
