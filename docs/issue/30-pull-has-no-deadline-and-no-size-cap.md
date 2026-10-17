# 30 · `--pull` has no deadline and no size cap: one slow peer turns the launch into an invisible process that refuses to quit

**Severity: medium** — the door is CLI-only (nothing in `ui/` reaches it, and
`pull_workspace` has exactly one caller), it needs a URL the user typed, and every
*wrong* answer it can get is handled with a message and a normal session. What it
does not have is a deadline. A peer that accepts the connection and does not hang up
is enough: the launch stops in the middle of starting, so the window is never built,
`--quit` is refused, `--auto-exit` never fires, and the only way out is
`taskkill //PID`. Second, the read has no cap, so the peer's size becomes the app's
working set and then its file. Third — the part that reaches past the CLI — the MCP
server is already listening when the launch stops there, so an agent's `tools/call`
either hangs or queues and lands later.

## Evidence

`pull.sh` (10 endpoint shapes, one launch each), `pullquit.sh` (quit probes on either
side of the pull, plus a window inventory), `pulldead.sh` / `mcpdead.js` (which MCP
method still answers), `pullqueue.sh` / `mcpwrite.js` (a call the client abandoned,
read back from the file), `pullwrite.sh` (both arms hard-killed), `pullabort.sh` (what
the next launch says about the kill). All against the sandbox exe, each on its own
`--db` under `.scratch/issue-test/ftest/pull-*/`, every probe gated on the 5880
listener being the pid the arm read out of its own `quire.log`.

The peer is `pullserver.js`, a mock on 127.0.0.1:5899 with one mode per endpoint
shape. `stall` accepts and never writes. `hold` writes the **complete, valid** body
and then keeps the socket open. `slow` trickles the same body over 30 s and closes.

**The control: a peer that answers and hangs up.**

```
── arm ok ──                                    (pull-arms.log)
  log pid=130836, first session-start line at +1998 ms
  working set MB every 2 s: 129.4 124.2 124.2 124.2 124.2 124.2   peak=129.4
  at +12 s: process=YES  5880 listener=130836
  console: quire: mcp server on 127.0.0.1:5880;quire: imported 14 pages from
           http://127.0.0.1:5899/api/export;dump-state: pages=33 …
  --quit: rc=0  output=quire: the running instance accepted the quit request  → process gone
  [{"pages":33,"blocks":242}]
  log tail: session started (pid 130836); session ended
```

**No deadline, and EOF is the only thing that ends the read.** `hold` is the case that
shows the client is not reading a response, it is reading a socket: the body it got was
the same 7,450 bytes the control got, every byte of it, and `read_to_string` still has
not returned 48 s later.

```
── arm hold (peer hold, --auto-exit 15) ──       (pullquit.sh, arm hold)
  [+3s] session pid=150964 job=765  5880 listener=150964
  [+11s] quit#a rc=1  proc=alive  out=Error: "the instance refused the request: it could not schedule its exit"
  at +48s (auto-exit said 15 s): 2886766||41.1 MB
  [+51s] quit#b rc=1  proc=alive  out=Error: "the instance refused the request: it could not schedule its exit"
  console: quire: mcp server on 127.0.0.1:5880;
  server:  [srv hold] request "GET /api/export HTTP/1.0" after 1 ms;
           [srv hold] sent the body, keeping the socket open (no EOF);
  log:     [info] session started (pid 150964);         ← one line. no end record.
  survived every probe → stopping my own pid 150964
```

`stall` (never writes at all) is identical except that nothing is even received:
working set pinned at `41 41 41 41 41 41`, `--quit` refused, process still alive,
`log lines: 1`.

**The window is never built, and that is a window inventory, not an inference.**
`winprobe.ps1` enumerates the pid's top-level windows and reads handle,
`IsWindowVisible`, title and rect — properties, no clicks, no pixels. Its control is
the same probe against the same exe in a session that got past the pull, and against
`explorer.exe` (which returns `vis=True` rows, so the probe is not blind).

```
arm ok,   +11s:  "Quire 速记"|560x340   "Quire"|1296x839   "Quire hotkeys"|16x16
                "Quire tray"|16x16   ""|16x16 vis=True   3× "Default IME"|0x0
arm hold, +12s:  "Quire hotkeys"|16x16   "Quire tray"|16x16   ""|16x16 vis=True   3× "Default IME"|0x0
arm hold, +49s:  (exactly the same six, unchanged)
arm slow, +14s:  (the same four as hold — no main window, no 速记 window)
arm slow, +42s:  "Quire 速记"|560x340   "Quire"|1296x839   "Quire hotkeys"|16x16 …
```

The last two lines are the causality: the peer closed at +30.2 s and the main window
appears in the next inventory. A stalled pull does not build a slow window; it does not
build one at all. `pullabort.sh` reduces the same fact to a count: main windows
`0` while held, `1` in the control.

**The refusal is exactly the pull, and it is honest.** The arm asked twice on each side
of the peer's close:

```
arm slow:  [+13s] quit#mid   rc=1  proc=alive  out=Error: "…could not schedule its exit"
           server closed? 1 — at +42s
           [+44s] quit#after rc=0  out=quire: the running instance accepted the quit request
           log: session started (pid 167960); session ended
```

So the message is the code's own designed answer (`main.rs:1328-1333` documents it:
`on_quit` replies `false` until `quitting` is set, which happens on the line before the
event loop starts), and `--quit` maps `err` to that sentence at
`src/platform/quit.rs:409`. Nothing lies to the caller. What is missing is any way for
the caller to make the process stop, and any way for the process to stop itself: with
`--auto-exit 15` it was alive at +51 s, because the auto-exit `Timer` is *constructed*
at `main.rs:1212` — 32 lines after the pull at `:1180` — so a blocked pull means the
timer is never created, not merely never ticked.

**The kill leaves a mark the next session reports.** `pullabort.sh`, same case dir:

```
── arm 1: hang, then kill ──
  hold: pid=161060 owns 5880, main windows=0
  at +20 s: main windows=0  --auto-exit said 25
  alive → taskkill //PID 161060 //F
── arm 2: the next launch in the same case dir ──
  02:09:41 [info] session started (pid 161060)
  02:10:15 [warn] the previous session aborted: did not shut down cleanly (killed, crashed natively, or lost power)
  02:10:15 [info] session started (pid 173908)
  02:10:29 [info] session ended
```

That warning is correct — the session *was* killed — and it is the whole record the
user gets of why Quire opened 34 s later with no window the first time.

**The agent door is up while the human door is not.** `pulldead.sh`, three methods,
20 s deadline each, against a listener proved to be the stalled session:

```
stalled (peer hold):  tools/list  answered in 13 ms (2394 B)
                      create_note no answer in 20013 ms
                      list_pages  no answer in 20015 ms
control  (peer ok):   tools/list  answered in 14 ms (2394 B)
                      create_note answered in 61 ms
                      list_pages  answered in 250 ms
```

`tools/list` is served on the HTTP thread; `tools/call` is turned into a channel job and
handed to a pump that is a Slint timer (`src/app/mcp_tools.rs:72-92`, repeated at
250 ms, draining `rx.try_recv()` inside the callback), and `arm_pump` runs at
`main.rs:1161` — before the pull. So the job has somewhere to be queued and nothing to
drain it. To an MCP client the endpoint is *alive*: it lists ten tools and then freezes.

**A call the client gave up on is not a call that did nothing.** `pullqueue.sh` writes
two `create_note`s with a 4 s client deadline while a 30 s peer still has the launch,
then asks the same door once more after it lands, then reads the file:

```
  [+6s] two create_note calls, client deadline 4 s each, main window: []
  duringpull-0: client gave up after 4010 ms
  duringpull-1: client gave up after 4005 ms
  [+49s] main window: ["Quire"|1296x839]  console: …imported 14 pages…
  afterpull-0: answered in 102 ms
  [{"queued_calls_that_landed":2,"after":1,"notes_total":3,"pages":33}]
  [{"id":1,"title":"duringpull-0"},{"id":2,"title":"duringpull-1"},{"id":3,"title":"afterpull-0"}]
```

Both abandoned calls executed, in order, as ids 1 and 2, the moment the pump started
ticking. An agent that retried on timeout gets the write twice — once from the retry
that answered, and once from the job nobody cancelled.

**And when the peer never closes, those queued writes are simply gone.** `pullwrite.sh`
hard-kills both arms instead of quitting them:

```
── stalled (peer hold) ──      pid owns 5880, mcp.json present=yes, no main window
  mknotes error: timeout                                  (3 calls, 10 s deadline)
  after hard kill: db=present
[{"notes":0,"pages":19,"mine":0}]
── control (peer ok) ──
  control: wrote 3 notes ids=1,2,3
  after hard kill: db=present
[{"notes":3,"pages":33,"mine":3}]
```

The control's three writes survive a `taskkill //F` (the debounced flush had its
600 ms and then some, matching `20`), so the stalled arm's `notes=0` is the queue, not
the flush.

**No cap on what the peer may send.** `big` streams 32 MB of frames at ~8 MB/s:

```
── arm big ──                                       (pull-big.log)
  working set MB every 2 s: 73 140.1 199.5 199.5 194.2 194.2   peak=199.5
  console: …imported 32 pages from http://127.0.0.1:5899/api/export;dump-state: pages=51…
  --quit: rc=0 → process gone
[{"pages":51,"blocks":177}]
  quire.db          67,481,600 B      quire.db-wal  68,466,192 B
  quire.db-shm          32,768 B      quire.db.bak1      212,992 B
```

A 32 MB peer costs 199.5 MB of working set against a 124 MB baseline, and leaves
~130 MB in the library directory, which is where it stays — the `.bak1` recovery point
in that listing is the *pre-pull* 212 KB file, so the first snapshot after this is the
one that has to carry it. `Content-Length` is read for nothing: `lan_client.rs:31-33`
accumulates into one `String` until EOF, and `:46-48` slices the body out of that same
string, so the peer's bytes exist twice before the first frame is even parsed.

**Everything it can answer, it answers well.** The seven arms whose peer responds and
closes all reach a normal session and quit with `rc=0`:

```
boom        HTTP 500            → quire: pull failed: HTTP 500: no such shareno such shareno such share
nohdr       junk, no \r\n\r\n   → quire: pull failed: malformed response (no header terminator)
nolistener  nothing on 5899     → quire: pull failed: connect 127.0.0.1:5899: 由于目标计算机积极拒绝，无法连接。 (os error 10061)
badscheme   ftp://…             → quire: pull failed: only http:// URLs are supported for LAN pulls
empty       http://             → quire: pull failed: empty host in URL
html        a web page          → quire: imported 0 pages from …      ← the one soft spot
```

`boom`'s message repeats the peer's body three times because the mock repeats it and
`lan_client.rs:47-49` truncates at 80 characters, not at a line. And `html` — a URL
pointed at any ordinary web server — reports success with zero pages and sets a
`db_notice` of nothing, because `controller.rs:8285` only notices `imported > 0`. One
line of stderr there would make a typo'd URL self-explanatory.

## Cause

- `core/services/lan_client.rs:23` connects with `TcpStream::connect` (the OS
  deadline, none of its own) and `:30-33` reads with `read_to_string`, whose only
  termination is EOF. Grep for `set_read_timeout|connect_timeout` across
  `lan_client.rs` and `lan_server.rs`: no hits. The HTTP/1.0 convention the header
  comment at `:17-19` relies on — "the server closes per request" — is an assumption
  about the peer, and the whole product is a *peer that has this code*: `28` measured
  that any machine on the segment may connect to 5877 uninvited.
- The call sits at `src/main.rs:1179-1189`, before `--dump-state` (`:1192`), before the
  auto-exit timer (`:1212-1230`), before `quitting.store(true)` (`:1332`) and before
  `slint::run_event_loop_until_quit()` (`:1340`). Everything that could end the process
  is downstream of the network read.
- `main.rs:1108-1166` binds 5880 and `arm_pump`s it *above* the pull, so the agent door
  opens into a session that cannot service it.
- `import_lan_pages` (`controller.rs:8242-8291`) writes through the normal `state.record`
  path, so a queued job that lands late is a normal write with a normal id.

## Not measured (said plainly)

**No real peer machine.** Every arm here dials 127.0.0.1:5899 on the same box. A
`--pull http://<neighbour>:5877` against a second Quire is still unperformed, and
`29`'s firewall rules are why. That does not weaken the deadline finding: the hang is
caused by a socket that is open and quiet, which loopback reproduces exactly.

**A release binary.** Every arm ran the debug exe, where the two `eprintln!` lines are
visible when launched from a terminal. Under `windows_subsystem = "windows"`
(`src/main.rs:6`) a session started by double-click has no console to put
`mcp server on 127.0.0.1:5880` on, so the user's whole experience of this defect is a
Quire that does not open and a process that must be killed. That half is read off the
cfg attribute, as in `25`, not measured.

**Not one write of product code.** The four lines this record would change are
`lan_client.rs:23` and `:30-33` (deadline, cap) and `main.rs:1179-1189` (where the pull
sits relative to the loop), all in the git dependency and the app's startup path.

## Candidate fixes

- **Give the read a deadline and a ceiling.** `stream.set_read_timeout(Some(…))` before
  `read_to_string`, plus a bounded loop: `read` into a `Vec<u8>` and fail at a cap
  (the workspace is a document tree; 8 MB is generous for one). Both are the same
  ~10-line shape and no new dependency, which is what `ADR-0081` priced when it said
  the gap in this client is transport security rather than "does it do networking".
  Respect `Content-Length` when present so a truncated body fails loudly instead of
  importing half a workspace.
- **Move the pull after the loop, or make it cancellable.** The cheapest version is a
  deadline as above; the honest version is that a network read has no business being
  between "we own the quit pipe" (`main.rs:483`) and "the event loop exists"
  (`:1332-1340`). Two shapes work: do the pull from a `SingleShot` timer once the loop
  is running (the same trick `--scene` already uses at `main.rs:1294-1300`), or spawn
  the read on a thread and have `on_quit` cancel it. Either way `--auto-exit` and
  `--quit` must be able to end a launch that is waiting on a socket.
- **Do not open the agent door into a session that cannot answer.** Either `arm_pump`
  and the bind happen after the pull, or `tools/call` refuses with a stated reason
  while `mcp_running` is true but the loop has not started. As measured above, the
  current combination is the worst of the three: `tools/list` promises ten tools, the
  call queues, and the queue survives a client that has already given up.
- **Say something when the answer is nothing.** `imported 0 pages` on an HTML page
  should be `the peer served no Quire frames (7450 bytes parsed, 0 pages)`. One
  `eprintln!` in the `Ok` arm of `main.rs:1182-1186`, next to the existing one.
- Keep what works: the five error strings are specific and each names the thing that
  failed, every error arm still opens a usable session, `--quit` never pretends success
  it did not get, and the whole-workspace write path flushes even through a hard kill
  (the control's 3/3 rows). A fix that trades the honest `err` answer for a silent
  success is worse than this.

## How this harness got it wrong

1. **`dribble` looked like a second permanent hang and was a timing artifact.** In
   `pull.sh` every arm asks `--quit` once, at +12 s. The 40-chunk peer finished at
   ~+19 s, so the probe hit the pull mid-flight and reported the refusal — a *true*
   answer to a question the record did not mean to ask. The re-run of the same arm
   shows `--quit: rc=0 → process gone`, and `pullquit.sh` was written specifically to
   put probes on both sides of the close. Lesson taken to the README: one probe per arm
   measures the probe's timing, not the product's behaviour.
2. **A leftover session turned the next launch into `14`'s modal.** After a mid-harness
   `exit 5`, one sandbox session stayed up; the next `pullinject.sh` printed
   `ABORT: no new session pid` and `rm -rf` said `Device or resource busy`. Before
   re-running anything, the harness now enumerates processes whose path matches
   `sandbox-20261017` and refuses to start while any exist, listing their pids so they
   can be killed by pid. It is the same rule `14` records for the product, applied to
   the rig.
3. **`winprobe.ps1` did not compile at first, and an empty result looked like "no
   windows".** `Add-Type` on PowerShell 5.1 rejects `new()` (target-typed `new` is a
   later C#); the fix is `new System.Collections.Generic.List<string>()`. The failure
   mode was silent — the function printed nothing, which is also what "this process has
   no windows" would print. Hence the `explorer.exe` control at the top of the
   evidence: if the probe cannot produce `vis=True` rows for a known GUI process, it
   gets no verdicts.
