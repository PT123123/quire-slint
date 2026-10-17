# 28 · one turn of the 共享 switch opens the whole workspace to the segment — permanently, and with no credential

**Severity: medium** — the share is opt-in, read-only and honestly labelled, and
this pass confirmed all of that. What is not opt-in is the *duration*: one turn
writes one settings row, and every later launch serves again without a flag, a
prompt or a line on the main window. The second half is that the socket answers
anyone who can reach it: no token, no pairing code, while its sibling on 5880
refuses 401 without a bearer.

## Evidence

Two scripts, both PID-gated (`lan.sh`, `lan2.sh`, `lan3.sh` + `lanprobe.js`,
`lanprobe2.js`, `exportscan.js`, `mkorg.js`), against a throwaway `--db` under
`.scratch/issue-test/ftest/lan2/` and the sandbox exe.

**The control says the gate is real.** A session launched without `--share`,
against a library whose `settings` table has no `lan.share` row:

```
5880 listener pid = 152324  (session pid 152324)
5877 listener pid = []            ← expected empty
5878 listener pid = 152324        (sync: always on, record 21)
console.log: quire: mcp server on 127.0.0.1:5880
```

**`--share` opens it.** `console.log` becomes
`quire: LAN sharing on 0.0.0.0:5877;quire: mcp server on 127.0.0.1:5880;` and the
5877 listener is that session's own pid. What the socket answers, all with no
credential at all:

```
GET /                      -> 200   169 B   (the help text, which is the API doc)
GET /api                   -> 200   169 B
GET /api/pages             -> 200   341 B   14 lines: "100\tWeekly Review", "103\tKeyboard Shortcuts\tunder #102", …
GET /api/export            -> 200  7450 B   (7014 characters, 184 lines, 14 page frames)
GET /api/page/100.md       -> 200   445 B   first line: # Weekly Review
GET /api/page/100          -> 200   445 B   (the ".md" is stripped, not required)
GET /api/page/999999.md    -> 404   "no such page"
GET /api/page/..%2f..%2f..%2fwindows%2fwin.ini -> 400 "bad page id"
POST   /api/pages          -> 405   "method not allowed (read-only share)"
DELETE /api/pages          -> 405   "method not allowed (read-only share)"
HEAD   /api/pages          -> 405
```

The traversal leg is genuinely closed: the id is parsed as an `i64`
(`lan_server.rs:109-115`), so `..%2f` cannot reach a path. The write legs are
genuinely closed, three methods including one that a "read-only" server should
answer with headers only.

**Templates stay in, and that is verified at all three doors** — the fix
`SPEC §三十八` describes is present and works:

```
db pages=19  non-template=14  template=5
/api/pages served ids (14): 100 … 113        list vs db: missing=none  extra=none
GET /api/page/114.md (template row) -> 404 "no such page"
GET /api/page/115.md (template row) -> 404 "no such page"
GET /api/page/116.md (template row) -> 404 "no such page"
/api/export mentions 0/5 template titles
```

**What the export carries** — `lan3.sh`, with the organizer seeded first so the
absence is measured rather than inferred (`mkorg.js` created 3 notes and 3 tasks
titled `shareprobe-*`; the library then held 10 notes, 3 tasks, 19 pages, 145
blocks):

```
pages in db=19  titles present in export=14     ← the 5 missing are the templates
notes in db=10  titles present in export=0
tasks in db=3   titles present in export=0
```

So the exposure is the document tree in Markdown, and not the organizer. The
page titles are the user's own page titles only in the seeded library; the body
is real content, starting `<<<QUIRE PAGE: Weekly Review>>>` and including every
block of the 14 non-template pages.

**The toggle is a fuse, not a switch.** `--share` alone writes nothing: the
`settings` keys after a `--share` session are
`builtin-templates-seeded, mcp.log, mcp.token, sync.device-id, sync.device-name,
window.h, window.w, window.x, window.y` — no `lan.share`. The other direction was
measured by writing the row the UI toggle writes, into a copy library, and
launching with **no `--share` at all**:

```
wrote lan.share=1  (insert into settings(key,value) values('lan.share','1'))
[persisted] session pid 163800
console: quire: LAN sharing on 0.0.0.0:5877;quire: mcp server on 127.0.0.1:5880;
5877 listener pid = [163800]   (session pid 163800)
… after the session:  [{"key":"lan.share","value":"1"}]
```

That is the documented behaviour — the row is what `main.rs:1050-1056` reads — and
the dialog label even says 「重启后生效」 (`SettingsDialog.slint:522`). The defect is
the *shape* of it: `lan.share` has exactly one reader in the whole UI
(`SettingsDialog.slint:524`, via `controller.rs:1682`), so the only place a person
can see that they are serving the library is inside the settings dialog, on a row
they have to scroll to. The runtime notice is one `eprintln!`
(`lan_server.rs:41`) and a release build is `windows_subsystem = "windows"`
(`src/main.rs:6`), which has no console. Recorded as a code fact, not a pixel
claim: I did not photograph the dialog in that state.

**The switch does not control the server in either direction.**
`controller.rs:5602-5606` is the whole handler:

```rust
ui.global::<UIState>().on_toggle_lan_sharing(move || {
    let on = !s.setting_flag("lan.share");
    s.record_setting("lan.share", if on { "1" } else { "0" });
});
```

No bind, no drop, no thread signal. So turning it OFF mid-session leaves the
listener running until the next restart, and `--share` with the switch OFF still
serves (`launch.share.or_else(...)`, `main.rs:1050` — the CLI wins). Half of that
is disclosed by the label and half is not. **Not driven:** reaching the toggle
means driving the settings dialog, which this pass does not do; the claim is read
off the two-line handler.

**Cost of serving it, measured.** 8 clients pulling `/api/export` back-to-back
for 3 s against the same session (`lanprobe2.js flood`):

```
one /api/export alone: 200 7450 bytes in 5 ms
flood 3 s: served=3102 (=1169.7/s) bytes=22 MB
errors: {}
mcp create before flood: 149 ms
mcp create during flood: 50, 252, 251, 249, 250, 250 ms
process CPU +6.16 s of 3 s wall on a 22-core machine
after flood /api/pages -> 200 341 bytes in 2 ms
```

No throttle, no cache: every request re-runs `repo.load()`
(`lan_server.rs:79`, `:116`, `:141`), so 1170 full workspace loads a second cost
about two of twenty-two cores. **This is a clean result, not a defect** — the
un-loaded control arm measured 248–251 ms per MCP create, so the 250 ms during the
flood is the UI-thread pump period (`main.rs`, MCP), not contention. There is no
DoS claim here; a neighbour can make the machine work, not stop it.

## Cause

- `lan_server.rs:36` binds `("0.0.0.0", port)` — every interface, not the one the
  user picked, and no bind-address setting exists.
- Nothing in `handle_connection` (`lan_server.rs:52-148`) authenticates: the route
  table is a `match` on the path and a 404 for anything else. Contrast the MCP
  server on 5880, which is loopback-only *and* bearer-tokened, and answers 401
  without it (this pass's own clean list).
- `main.rs:1050-1069` decides serving from `launch.share.or_else(|| if
  state.setting_flag("lan.share") …)` — a persisted row alone is sufficient, at
  every launch, forever.
- The on-screen state comes from the same row read once at startup
  (`controller.rs:1682`) and the toggle writes the row without touching the
  server (`controller.rs:5602-5606`).

## Not measured (said plainly)

**No remote host handshake.** `GET http://192.168.5.34:5877/api/pages -> 200
341 bytes` proves the listener accepts on the WLAN interface, but the connection
originated on the same machine, and Windows does not filter that. Whether a
neighbour actually gets in is decided by the firewall, and this machine's rule set
is the subject of record 29 — which enumerates an inbound-Allow, any-port,
Public-profile rule for exactly this kind of exe. Read 29 before treating this as
"reachable from the cafe", and read it before treating it as "not reachable".

## Candidate fixes

- **Put the fact on the window.** The cheapest honest version: while a share
  thread is alive, the main window should say 正在局域网共享 · 5877 · 14 页. Nothing
  does today, and that is a grep, not a guess: `lan-sharing` has exactly one
  reader in the whole UI (`SettingsDialog.slint:524`), and the other service-shaped
  string the shell writes at startup — `renderer-name` — is likewise drawn only
  inside that dialog (`SettingsDialog.slint:732`). Feed the line from the running
  server, not from the settings row, which is what makes the OFF-leaves-it-running
  case visible instead of merely wrong.
- **Give the socket a credential, like the MCP server has.** A read-only share
  still needs to know who is reading: `sync.device-id`/`sync.device-name` already
  exist as rows, so a pairing code shown in the same dialog and required as
  `?key=` or a header is a small change to `handle_connection` and no new crate.
  If the design answer is "the LAN is trusted", then that sentence belongs in
  `DECISIONS.md` next to the label, because the label says 只读 and nothing says
  任何人.
- **Scope the bind.** Offer the interface (or at least honour a
  `lan.bind`/`lan.port` setting through `setting_str`, which already exists for
  the MCP port) instead of hard-coding `0.0.0.0` + `DEFAULT_PORT`.
  Two corrections this record needed after `36` measured the flag surface: the port
  is *not* unreachable from outside — `--share-port` (`main.rs:150-154`) feeds the
  same `launch.share` slot that `main.rs:1050` reads first, and
  `--share-port 5891` was measured binding `0.0.0.0:5891` (`--share-port 0` binds an
  ephemeral port and prints it). What is missing is the **settings row**: there is no
  `lan.port` anywhere in `src/` or `ui/`, so the persisted-`lan.share` branch at
  `main.rs:1051-1052` can only ever be `DEFAULT_PORT`, and the dialog's switch has no
  way to change the number it advertises. Also: `--share-port` parses into a `u16` and
  silently leaves `launch.share = None` on failure, so a typo'd port does not turn the
  share off — it falls through to the `lan.share` row and serves `0.0.0.0:5877`
  (`36`'s `revert` arm, measured against a library seeded with `lan.share=1`).
- **Make the switch an action.** Either start/stop the thread in
  `on_toggle_lan_sharing` — the drop of the `TcpListener` is the stop — or make
  the row expire: write it with the session id and require a re-turn after each
  restart, so the one-time turn is not permanent. `--share` and the row must not
  be able to disagree with what the window says.
- Whatever is chosen, keep the four things this pass found working: the 405 on
  non-GET, the `i64` id parse that kills traversal, the template exclusion at all
  three doors, and the local write path's indifference to the flood. A fix that
  re-fails any of those is worse than the current shape, and
  `lanprobe.js`/`lanprobe2.js` are ready to become that gate.

## How this harness got it wrong

Recorded so the next pass does not repeat it, and because the mistake was the
only reason the numbers above exist at all.

1. **`start()` read a stale pid and the gate refused everything.** The first run
   of `lan.sh` printed `GATE: 5877 listener pid 171140 != this session's 173880`
   three times and measured nothing. `sed … | tail -1` over `quire.log` returns
   the *previous* arm's `session started` line, because the file already holds it
   before the new process writes its own. Fixed by counting the lines before
   launch and waiting for the count to rise, then cross-checking that the 5880
   listener **is** that pid (`start()` in `lan.sh:16-38`). The gate was the point:
   a probe that speaks to port 5877 must first know whose port it is.
2. **The single-page leg silently skipped.** `lanprobe.js all` regexed
   `/api/page/([0-9]+)\.md` out of the `/api/pages` body and reported
   `page ids seen: 0` — the listing is `id<TAB>title`, not URLs. A leg that finds
   nothing and continues is a leg that reports clean. Fixed in `lanprobe2.js` by
   splitting on tabs and asserting the served id set against `select id from pages
   where template=0` — which is where `missing=none extra=none` came from.
3. **A verification line threw after the write it was checking.** In `lan2.sh`,
   `db.prepare("select … where key=?", "lan.share")` raised
   `TypeError: The "options" argument must be an object` (node:sqlite binds
   values at `.all()`, not at `prepare`). The `INSERT` on the line above had
   already run, and the arm stands on two independent legs — the row read back as
   `1` after the session, and the listener bound without `--share` — not on the
   line that threw.
