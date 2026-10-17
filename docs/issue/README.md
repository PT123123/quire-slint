# Issues found by a test pass — 2026-10-17

Source: one run-then-test pass over `quire-slint` at v0.1.36, working tree dirty,
no file under `src/`, `ui/`, `benchmarks/`, `install/` or `justfile` modified by
this pass. Everything below is a *report*; the only writes were under
`.scratch/issue-test/` (gitignored analysis scripts and captures) and one copy
described in `01`.

Method, in short: build → test suite → headless sweep of all 186 declared scenes
→ pixel diffs against controls → motion/clip captures → interaction shots
(`--click` / `--key` / `--hover`) → live-process tests (`--quit`, second launch,
`--dump-state`, MCP HTTP, idle CPU) → static scans (theme table, `animate`
census, scene list).

Every claim carries the command that produced it. Where a claim rests on a pixel
diff, the diff's positive control is stated next to it, because most of the false
defects this repository has produced came from reading a diff without a control.

## The functional pass (17–42)

`17`–`32` came from a second pass on a different surface: not the renderer but
what the program *does* — the MCP server, the persistence pipeline, the sync and
LAN sockets, the storage failure paths, and the two doors at the ends of the wire
(`--share` writing a frame, `--pull` reading one back). `33`–`36` extend it to the
doors a person actually uses: the organizer's tag grammar, the `--open` import
path (encoding, size, and what the window says when either fails), the file
association, and the command-line parser itself. `37` goes back to the sockets with
the questions `21`/`22` left open — what a declared length costs before any body
byte arrives, and what a refusal does to the bytes already sent. `38`/`39` go to
the two ends of the *document* rather than the wire: they build a table and a
database inside the app through its own commands (`--scene`), then ask what the
export door writes and what the import door reads back. `40`/`41` point the same
door at media and follow it twice — the Markdown doors write a picture as a link
to a file that no door serves (`40`), and the sync port serves the file itself to
anybody who asks (`41`). `42` goes one layer under the search panel and asks what
the FTS5 mirror is told: a block moved to another page keeps its old page in
`search_blocks`, and the page's later deletion makes `prune` drop the mirror rows of
blocks that are still in the document. It ran against a copy of the
tree at `sandbox-20261017/quire-desktop`, so that the working tree, which carries 41
uncommitted edits of the user's (measured: `git status --porcelain | wc -l` at HEAD `444066f` —
the same number as the last record is a coincidence), was never built over or written to.

Method: launch the debug `quire-slint.exe` with `--db <case>/quire.db` and
`--auto-exit`, drive it over raw TCP (`node:net`, never `fetch` — the HTTP layer
under test is hand-written and `fetch` retries, normalises and hides exactly what
these arms look at), and read the answer back **from the file**, by copying
`quire.db` + `-wal` + `-shm` into a snapshot directory and querying that with
`node:sqlite`. Nothing in these records is asserted from what the session said
about itself when the question is whether the session can be trusted.

Three rules the first versions of these harnesses broke, and which is why
`17`, `20` and `25` each carry a "how the harness got it wrong" section:

- a stale `mcp.json` is not proof the listener is up — wait for a TCP connect;
- an exclusive handle must be released before the snapshot copy, or the read-back
  comes back empty and looks like the bug;
- the path handed to PowerShell must go through `cygpath -w`, not `sed`, and
  `cygpath -w` does not absolutise — use `-wa`.

Three more were paid for by the `33`–`36` arms, and each is the reason a number in
those records exists only after a re-run:

- **`$!` is a bash job number, not a Windows pid.** The cleanup that used it
  silently killed nothing, and the refused second launch was found two and a half
  minutes later still holding its modal box (`14`'s second `.md`-door run);
- **`printf '%s\n' "$@"` puts every argument on its own line**, so a `sed` that
  matches `--auto-exit 8` as one line never matched, the wait-for-exit branch never
  ran, and the first `flagprobe` batch reported "still alive, taskkill" for sessions
  that had exited cleanly by themselves (`36`);
- **a control must open the same library as the arm it controls.** One arm's case
  directory was named differently from the path the seed wrote, so `run()` built a
  brand-new library and the control contradicted the arm above it — caught because
  the two reported different `mcp.token` values (`36`'s `revert`/`retnothing` pair).

Every network arm that touches a live port gates on the listener PID belonging to
its own session and refuses to run otherwise (`syncprobe.js`, `syncpair.js`,
`lan*.sh`, `cliprobe.sh`, `flagprobe.sh`), and no arm ever pointed at
`%APPDATA%\Quire\quire.db`. The `42` arms added the same gate for the UDP discovery
socket: `announce.js` broadcasts a mock peer into the pairing table, so it first reads
`netstat -ano -p udp` and aborts unless 5879 belongs to the session it just started.

## Records

| # | severity | one line |
|---|----------|----------|
| [01](01-sweep-photographs-a-stale-shot-binary.md) | high | `sweep.ps1` builds nothing and reads `target\debug\quire-shot.exe`, a path every recipe stopped writing to; the file there was 11 days older than the tree |
| [02](02-gates-print-artifacts-instead-of-checking-them.md) | medium | the release and dist gates record PASS while printing an empty file size, because the artifact check is an interpolation, not an assertion |
| [03](03-sweep-list-declares-settings-twice.md) | low | the scene list has 187 rows and 186 names; `settings` is declared twice |
| [04](04-sync-scenes-photograph-the-appearance-tab.md) | high | `sync`, `sync-stats`, `sync-log` and `settings` render byte-identical PNGs — the sync scenes open the dialog but never move to the 同步 tab |
| [05](05-default-dark-dark-settings-are-one-picture.md) | medium | `default`, `dark` and `dark-settings` are one image: the sweep renders "default" with no `--scene`, and `dark-settings` has no arm |
| [06](06-tasks-list-rename-renders-nothing.md) | medium | `tasks-list-rename` is byte-identical to `tasks-list` at three window sizes; the inline rename never appears |
| [07](07-interaction-shots-never-pump-the-animation-clock.md) | high | `--click`/`--key`/`--hover` re-render without advancing `CLOCK` or pumping animations, so every interaction shot captures animated properties at their start value |
| [08](08-animation-coverage-is-4-of-60-blocks.md) | medium | 60 `animate` blocks exist, `apply_motion` knows 4 of them and `clips.ps1` gates those 4; 15 components contain none, so every overlay arrives with no transition |
| [09](09-settings-dialog-cannot-be-closed-with-escape.md) | high | Escape changes 0 pixels in the settings dialog: its only key handler sits on a `TextInput` that does not have focus |
| [10](10-popup-dismissal-is-invisible-to-the-harness.md) | medium | ten Escape tests, and none of them is readable: six move 0 px (three with no cause in the code) and four change the picture without proving a close, because `probe()` prints 7 of the 24 `*-open` flags |
| [11](11-quire-shot-panics-below-the-minimum-window-size.md) | medium | below 940×600 the shot binary dies inside Slint's software renderer, so no narrow-window shot exists at all |
| [12](12-no-swept-scene-overflows-the-viewport.md) | medium | `--scroll-y` moves 0 px on all 12 sampled scenes and works at `--h 600`, so the swept corpus never photographs a scrolled editor |
| [13](13-measure-startup-emits-nothing.md) | high | `--measure-startup` prints no `first_paint` record on either profile, which is the line `startup_bench.ps1` and PERFORMANCE.md parse |
| [14](14-second-launch-blocks-on-a-modal-message-box.md) | high | a launch while a session is running stops at a modal `MessageBoxW` with no timeout and no suppressing flag, and outlives the session it collided with |
| [15](15-palette-contrast-under-aa-in-three-themes.md) | low | `light`/`jade`/`crimson` carry secondary text and status colours below 4.5:1, measured from the shipped palette table |
| [16](16-docs-name-artifacts-that-are-not-built.md) | low | README:36's reproduce command and `Cargo.toml`'s binary count both describe the tree as it was before the rename |
| [17](17-a-refused-create-task-still-leaves-the-row.md) | high | `create_task` mints the row before it validates the title and the priority, so a refused call still leaves a blank/`none` task — and an unknown `list_id` is accepted as `0` |
| [18](18-due-answers-ok-to-a-date-it-refused.md) | medium | `update_task {due:"明天"}` answers `{"ok":true}` and changes nothing; `2026-13-45` and `0000-00-00` are stored verbatim and sort as text |
| [19](19-the-bin-is-not-a-boundary-for-the-agent.md) | medium | the agent can rewrite a row in the 回收站, and `trash_note`'s answer for the same row twice depends on which second it lands in |
| [20](20-the-600ms-window-opens-wider-on-every-read.md) | high | measured loss window: 10 creates 0 ms before a hard stop leave 0/10 in the file, and a read arriving every 300 ms keeps the queue open indefinitely because every MCP call — reads included — writes a settings row |
| [21](21-the-sync-port-answers-to-any-lan-peer.md) | critical | 5878 binds `0.0.0.0` with no credential: one datagram mints a pairing, `GET /sync/snapshot` exports the whole library, and `{"notes": []}` deletes it |
| [22](22-one-request-header-stops-the-app.md) | high | `Content-Length: 1099511627776` on 5880 (no token needed) is `vec![0u8; len]` + `read_exact` → the process dies on an allocation failure with no `session ended`, and the working set climbs to 1,344 MB on demand |
| [23](23-the-mcp-transports-four-small-doors.md) | medium | `bearer` lowercase 401s, 404/405 are answered before auth, `tools/call` without `id` answers `202` and does nothing, and a newline in a title is stored verbatim |
| [24](24-sequential-mcp-calls-cost-the-pump-period.md) | medium | sequential MCP calls are paced by the 250 ms UI-thread pump — 20 creates in 5.2 s, while 12 concurrent ones all land in one tick |
| [25](25-memory-mode-is-silent-and-discards-the-session.md) | high | when the library cannot be opened the session shows an empty document, reports every edit as saved, throws them all away at exit, and the only trace is one `eprintln!` a release build has nowhere to print |
| [26](26-the-recovery-point-lags-and-two-backup-facts-are-never-shown.md) | medium | the recovery point advances at most once per ten minutes of writing and never at shutdown — 2 durable rows sat 631 s behind a `bak1` that held 0 — and `backup_failed` / `take_snapshot_error` are read by nobody in the shell |
| [27](27-palette-does-not-answer-latin-and-f2-is-unbound.md) | medium | 22 of 25 Latin queries return nothing from the palette because it filters Chinese names only, `F2` is advertised with no `KeyBinding` behind it, and an empty result collapses the panel to 40 px |
| [28](28-a-one-time-share-toggle-opens-the-workspace-to-the-lan.md) | medium | one turn of 局域网只读共享 writes a row that re-serves the whole document tree on `0.0.0.0:5877` at every later launch with no flag and no credential, while the switch itself cannot start or stop the server |
| [29](29-130-inbound-firewall-rules-are-quires.md) | medium | 130 of this machine's 250 user-answered inbound firewall rules name a quire exe — any port, any remote address, Public profile — 37 of their 65 paths are already deleted, and the always-on `0.0.0.0:5878` is what keeps asking |
| [30](30-pull-has-no-deadline-and-no-size-cap.md) | medium | `--pull` reads to EOF with no deadline and no cap: a peer that never answers holds the launch open for its whole timeout (`--auto-exit` inert, `--quit` refused, MCP `tools/call` unanswered) and a 64 MB body costs 199 MB of working set |
| [31](31-the-lan-export-frame-is-a-one-way-format.md) | medium | the frame format is one-way: depth 4 and 5 both write three `> ` prefixes, `>` and `\n` are replaced by spaces in titles, every imported page lands with `parent=null`, and text outside a frame is discarded with no count |
| [32](32-a-page-that-quotes-the-frame-format-forges-pages-on-import.md) | medium | a `code` block that quotes the frame format arrives as an extra page, an emptied code block and a lost sentence — the splitter is line-oriented and fence-blind, and only titles are sanitised |
| [33](33-the-organizer-mcp-doors-speak-two-tag-grammars.md) | medium | `create_note` and `create_task` take the same `tags` string and store different sets — `Machine Learning` is two tags on a note and one on a task, an edit to any other field re-splits a task's own tags, and `notes.id`/`tasks.id` are independent sequences so one id numbers two tables |
| [34](34-a-non-utf8-markdown-is-refused-with-no-word-in-the-window.md) | medium | a GBK file, a UTF-16 file with a BOM, a 200 KB png, a directory and a missing path all leave the library byte-for-byte untouched and the window says nothing: `import_from_path` fails with one `eprintln!` that a release build has nowhere to print, and `set_db_notice` sits on the success path |
| [35](35-one-big-open-costs-minutes-now-and-seconds-forever.md) | high | `--open` of a 100,000-line markdown runs `import_from_path` on the UI thread before the event loop exists: 128 s with a window that is on screen and `Responding=False`, MCP never opened, `--quit` refused — and that library then costs 38 s on every later launch. The control (same 5.5 MB on one line) is free, so the cost is per block, not per byte |
| [36](36-a-flag-value-the-parser-cannot-read-is-a-flag-never-passed.md) | medium | every coercion in the flag parser is `parse().unwrap_or(default)`, so `--auto-exit abc`/`-5`/`0`/`abc=8`/a trailing `--auto-exit` all mean *run forever* and print nothing, `--share-port notanumber` does not close the share but reverts it to `0.0.0.0:5877`, `--no-mcp` is swallowed by `_ => {}`, and `--blocks 4000000000` aborts the launch on a 1.44 TB allocation |
| [37](37-a-declared-body-length-is-paid-for-in-ram-before-a-byte-arrives.md) | high | both hand-written HTTP doors size a buffer from the client's `Content-Length` and then wait (`mcp.rs:243-244`, quire-core `sync/server.rs:60,62`), with no cap and no read deadline: one socket declaring 200 MB and sending nothing moves the working set 130 → 315 MB and holds it, three on the credential-free sync port reach +286 MB in seven seconds — and every refusal answers and closes with the body still unread, so twelve identical chunked requests split 7 answered / 5 reset |
| [38](38-a-database-block-exports-as-one-empty-line-through-every-network-door.md) | high | the app builds a database with 4 columns, 5 records and 20 values and the library keeps all of it, but `GET /api/page/<id>.md` exports it as **one empty line** and `/api/export` carries the words `Name`/`Points`/`Done`/`Due` zero times — because the share and the frame call `export_page`, which hard-codes an empty database reader (`export_service.rs:37-39`), while the two desktop doors pass the real one (`controller.rs:8428`, `:8567`) |
| [39](39-the-app-exports-its-tables-in-a-syntax-it-refuses-to-read.md) | medium | the export door writes its grid as GitHub-Markdown and the import door refuses that grammar: the same file comes back as three literal paragraphs — including the invented `| --- | --- | --- |` rule row — with `kind='table'` rows 0, and the exporter's `|`→`\|` escape is deleted by the importer's prose pass, so `A|B` leaves protected and returns as a raw column separator sitting in a paragraph |

| [40](40-the-share-exports-a-picture-as-a-link-to-a-file-it-will-not-serve.md) | medium | `--scene image` stores a real 146,748-byte PNG with a real row, and `GET /api/page/102.md` exports it as `![sample-gradient](quire://attachment/1)` — an address the share has no route for (four shapes answer `not found`) while `/api/page/102.png`, `.exe` and `.tar.gz` all answer `200` with the page's own markdown (`md5=5fcd74e97a` ten times over, because the id parse is `split('.').next()`). Pull the frame back and the picture is a paragraph reading `!sample-gradient` on a hyperlink to an id from somebody else's library: `attachments` rows 0, bytes 0, and no code anywhere matches `quire://attachment/` |
| [41](41-the-sync-port-hands-out-every-attachment-s-bytes-to-any-peer.md) | critical | `GET /sync/attachment/<id>` on the credential-free `0.0.0.0:5878` reads the file out of `<library>/attachments/` and sends it: `/sync/snapshot` first names all three (`1:sample-gradient:image/png:146748:1.png` …), then three plain GETs return bodies whose md5s equal the source files, and ten repeats of the 1.8 MB one cost 534 ms each with no token, pairing or limit. Six junk ids that answered `404` each appended a line to the persisted `sync.log` — peer always `"对端"`, clock at minute resolution — so an unauthenticated reader also writes the user's audit trail |
| [42](42-a-moved-block-keeps-searching-for-the-page-it-left-behind.md) | high | `search_blocks` stores which page a block is on, and `Change::BlockMovedToPage` — the only statement in the repository that re-keys `blocks.page` (`repository.rs:867`) — never writes that column. One moved block leaves one stale row; a five-block page leaves 5 of 5, the destination page's own find returns `[]` and the page it left still returns every hit under its old title. Delete that page and `prune` (`search_index.rs:162`) drops the mirror rows of five **live** blocks: restart does not heal it, a plain sync round does not, and an edit heals exactly the block edited (4 of 5 still missing) |

## What came back clean

Recorded so the next pass does not re-spend time on it:

- `cargo check --workspace --all-targets`, `cargo test --workspace`,
  `cargo build --release` — green, zero warning lines.
- 186 swept scenes, 0 `RENDER-FAIL`.
- 12 palettes all render distinct pixels; an unknown `--theme bogus` and an empty
  `--theme ""` both fall back to `midnight` (0 px difference between them), which
  is the documented resolution in `Theme.slint`.
- `--quit` against a live session: exit 0, `the running instance accepted the
  quit request`, `session ended` in the log 5.1 s after `session started`.
- `--dump-state` on the throwaway db: `pages=19 blocks=30`.
- MCP: `quire: mcp server on 127.0.0.1:5880`, 10 tools listed, 401 without the
  bearer token, 404 on an unknown path, and a real create + list round trip.
- Idle cost: 0.47 % of one core, Working Set 123.9 MB.
- `just clips`: 4/4 motions pass their distinct-frame floor.
- Window reflow at 1024×768: all 11 sampled scenes re-lay out, none clip.
- Hover does render — the sidebar row's non-animated icon swap appears under
  `--hover` (262 px, bbox x=18..57 y=163..174). Only the *animated* half of the
  same hover is missing, which is `07`, not a dead pointer.

From the functional pass:

- Well-formed MCP traffic round-trips: `create_note`/`list_notes`/`update_note`
  and the trash/restore pair all produce the row the file then carries, ids
  included, and 10 tools are listed with `tools/list`.
- CJK and emoji titles survive the whole path. The `latin1` mismatches an earlier
  battery reported were the harness decoding responses as latin1; re-read as
  UTF-8, the stored text is exactly what was sent (`23`'s correction note).
- The debounced write does land: the control arm of `20` (10 creates, 900 ms of
  quiet, then a hard stop) shows 10/10 in the file.
- `--quit` against a live session is graceful on every functional arm too, and
  after `--quit` the queue is empty — the loss in `20` needs a kill or a crash.
- Corruption recovery works as designed and is careful about the corpse: the bad
  file is moved to `quire.db.corrupt` rather than deleted, each candidate snapshot
  is validated by opening it, and the restored state is snapshotted again at once
  (`core/storage/backup.rs:222-262`). What `26` reports is how old the fallback is
  and what it fails to say, not that the mechanism is unsafe.
- `.bak1` is a real `VACUUM INTO` copy that includes WAL content: measured 253,952
  bytes carrying 2 rows while the main file was 4,096 bytes with the rows still in
  the `-wal`.
- 5880 binds `127.0.0.1` only, and refuses a request with no or wrong bearer
  token (401) — `22` and `23` are about what happens *after* the token check or
  before it on paths that do not touch data.
- The palette's keyboard Enter resolves the highlighted row correctly:
  `controller.rs:2890` indexes `s.commands`, and `set_query`
  (`state.rs:4842-4853`) writes the *filtered* list into that same `VecModel`.
  `palette_action` has one arm per command and no wildcard (`state.rs:14966-14984`).
- The LAN share is read-only in the literal sense: `POST`, `DELETE` and `HEAD` all
  answer `405 method not allowed (read-only share)`, and the page id is parsed as
  an `i64`, so `/api/page/..%2f..%2f..%2fwindows%2fwin.ini` answers `400 bad page
  id` rather than a file (`28`).
- Templates are excluded from all three share doors, and verified against the file:
  `/api/pages` served 14 ids where the library holds 14 non-template pages
  (`missing=none extra=none`), each template id answers `404 no such page`, and
  `/api/export` names 0 of 5 template titles. The organizer is not carried either:
  10 note titles and 3 task titles in the same library appear 0 times in the export.
- Serving it is cheap and does not starve the local write path: 8 clients pulling
  `/api/export` back-to-back took 3102 responses in 3 s (1170/s, 22 MB, 0 errors)
  for +6.16 s of CPU, while MCP create latency stayed at the same 248–251 ms the
  un-loaded control arm measured (`28`'s flood leg).
- `--share` writes no settings row, so the CLI door cannot leave the library
  sharing after the process is gone (`28`).
- The pull *client* is careful about a peer that answers badly: of ten endpoint
  shapes (`30`), seven — HTML, a non-HTTP response, a 500, a listener that never
  accepts, a bad scheme, an empty URL, a body dribbled at 400 B/s — end with a
  readable `quire: pull failed: …`, a session that still quits, and a library that
  still holds its own pages.
- Everything the frame carries *inside* a frame survives: six paragraphs of a
  five-deep chain landed byte-for-byte (`31`), `写作与中文测试` round-trips
  untouched, and the destination keeps its own organizer rows untouched — the
  importer only adds pages.
- The template filter holds on the import side too: `controller.rs:8269` writes
  `template: false` for every page it creates, so a peer cannot import a template
  (`32`).
- A committed library cannot hold an orphan page, so `frame_workspace`'s recursive
  tree walk cannot go blind on a dangling `parent`: `pages.parent` is
  `REFERENCES pages(id) ON DELETE CASCADE`, enforced by the app
  (`core/storage/database.rs:69,85`) and by `node:sqlite` — the attempt to seed one
  was refused with `FOREIGN KEY constraint failed` (`32`).
- The organizer's read doors are honest: `include_trashed` behaves the same on notes
  and tasks (2, then 1 and 2 with `"trashed":true` after a trash),
  `update_note {tags:""}` really clears to `[]`, `due:""` stores NULL rather than
  `""`, `done:false` leaves `completed_at` null, `priority:"HIGH"` is lowercased on
  the way in, and note bodies keep newlines, tabs and Markdown-looking text
  verbatim (`33`'s leg E).
- The `--open` door parses what it accepts: `good.md` arrives as three blocks
  (`heading_1`, one `paragraph`, one `todo`) on a page titled `good`, and the bare
  positional form behaves exactly like `--open` (`34`).
- A refused import is a refusal, not a partial write: the control and all five
  refusals (GBK, UTF-16 with BOM, a png, a directory, a missing path) return the
  same census — 19 pages, 145 blocks — so nothing half-landed (`34`).
- The two-minute import loses nothing when it finishes: 100,000 lines become
  `pages=20 blocks=100145 block_children=100145` with `on_newest=100000`, and the
  `--quit` that follows drains in 1.1 s with `session ended` in the log (`35`).
- `--share-port` is not decorative where it parses: `5891` binds `0.0.0.0:5891` and
  prints that line, and `--share-port 0` binds an ephemeral port and prints the
  number *actually* bound (`0.0.0.0:54932`) rather than the `0` it was given (`36`).
- `--auto-exit 8` is honoured: all eleven arms that ask for 8 seconds exit between
  9.59 s and 10.13 s after launch, `session ended` in the log — the timer itself is
  not the defect, only its parse is (`36`).
- A launch that dies on its own still leaves a well-shaped corpse: the
  `--blocks 4000000000` abort opened the library, created all 26 tables and wrote
  zero rows — no half-populated state for the next session to inherit (`36`).
- The MCP door matches header *names* case-insensitively: `Content-Length`,
  `Content-length` and `CONTENT-LENGTH` each wrote one note and each returned the
  new id (`pages=19 blocks=145 notes=3` read back from the copied library). The
  sync door does not, which is `37`'s finding, not this one.
- On 5880 the credential really does precede the allocation (`37` C): the four
  refusal arms — no header, wrong bearer, right token with `GET`, right token with
  `POST /nope` — each move the working set by 0.0 MB and each get a complete
  `401`/`405`/`404` with `fin=true`.
- The parked memory is not a leak and not a stall (`37` B/C/E): 411.2 → 125.0 MB
  and 35 → 24 threads when the clients close, `tools/list` in 2–4 ms the whole time,
  and `--quit` graceful in all six modes (`rc=0`, `alive_after_quit=no`, one
  `session ended` line).
- The sync door's refusals are precise where they reach a parser:
  `bad snapshot: missing field `version` at line 1 column 14` for the two header
  spellings it recognises, `EOF while parsing a value at line 1 column 0` for the
  five it does not — all seven answered `400` with a body in the kept run (`37` E).
- The media layer itself is sound. Three sessions each claiming one attachment id
  produced three rows and three distinct files on disk, `2.pdf` holding exactly the
  scene's 1,842,000-byte payload, `mime` derived from the extension, `width`/`height`
  from the decode, and `blocks.attachment` set on all three rows across graceful quits
  (`40` A). The id watermark that makes this possible is seeded `max(id)+1` at load
  (`state.rs:901-930`, `:3098-3102`) and the sync merge draws from the same counter
  (`:15815-15826`), so a renumbered remote row cannot collide with a local file.
- The share's guard rails held in the same battery that found the loose one: template
  id 114 answered `no such page` rather than its markdown, `102abc` and `.md` answered
  `bad page id`, `999`/`0`/`-1` answered `no such page`, and `/api/export.png`,
  `/api/attachment/1`, `/attachments/1.png`, `/1.png` all answered `not found` (`40` C).
- The importer's link grammar works: `[text](url)` came back as a paragraph carrying a
  `link` mark whose `url` is byte-exact (`marks.start=0,end=20,url=quire://attachment/2`),
  which is why `40`'s complaint is the unresolvable target and the stray `!`, not a
  dropped line.
- The sync *receiving* half is careful on purpose: `sync_stored_attachment`
  (`state.rs:15203-15224`) rebuilds `file` as `{id}.{ext}` from this device's own id and
  clears `thumb`, because the peer's names are network input, and an image buffer is run
  through the store's real `import_bytes` (`:15938-15950`). `41`'s exposure is the sending
  route, not a missing sanitiser.
- `GET /sync/attachment/<id>` refuses what it cannot resolve: six malformed ids (`0`,
  `999`, `1.png`, empty, `-1`, a 20-digit number) all answered `404 no such attachment`,
  because the id is `parse::<u64>` with `unwrap_or(0)` and there is no row 0 — the one
  checked thing on that path (`41` D).
- The app's own complex blocks persist. `--scene table` and `--scene database-table`
  leave a 3×2 grid (6 `table_cell` rows) and a database with 4 properties, 5 records
  and 20 values in the file after a graceful `--quit`, read back from the snapshot —
  the loss in `38` is at the door, not in storage (`38` A).
- `--scene` is a usable non-UI door for structure: it runs the app's real commands
  600 ms after launch (`main.rs:1310-1325`) and writes through the same debounced
  pipeline every other arm exercises, so a scene-built block is indistinguishable in
  the file from a hand-built one.
- The export door's table *syntax* is a correct GitHub table: header row, `| --- |`
  rule row, cells in store order, and a diff against the no-table control shows only
  the intro paragraph replaced by those three lines — `39` is about the importer, not
  about a malformed writer (`39` B).
- Import of a table with no pipe and no newline in any cell is exact: the same three
  lines land as three paragraphs, which is the case the importer's docstring
  actually describes. The failures in `39` are the escaped-pipe cell and the rule
  row, not the whole grammar.
- The database block round-trips through the sync door unchanged (`merge.rs:562-587`,
  `engine.rs:403`): `38` is scoped to the three Markdown-writing doors, not to the
  protocol that carries the row.
- The mirror starts clean and stays in step on every path but one. Both seeded libraries
  report `with_text == indexed` (129/129 and 131/131) and `page <> page_id` count 0, and after
  `--open`, `--scene`, MCP writes, `PageTitleSet`, `BlockTextSet` and deletes the same two
  numbers still agree — `BlockMovedToPage` is the only re-keying change in the repository
  (`grep 'UPDATE blocks SET page ='` answers one line, `repository.rs:867`), so `42` is one
  missing call, not a category of them.
- Every other search door is drift-immune because it does not read the index. Ctrl+F builds its
  session from `doc.borrow().page_blocks(page)` (`state.rs:2247-2250`) and "does no IO"
  (`find_service.rs:41-57`); `Workspace::search` (`workspace.rs:839`) scans the in-memory blob;
  the database view's own search is a live `INSTR` predicate inside the view's statement
  (`storage/database_query.rs:444-460`). The consequence in `42` is scoped to the FTS5 mirror.
- The index's own query is careful about what it exposes: `matches` filters templates on the
  read path (`search_index.rs:289`, `p.template = 0`), over-fetches `limit*4` clamped to
  20..500 so ranking has room, and `SearchService::search_async` never blocks the UI thread —
  `poll` yields `None` while the worker runs (`search_service.rs:76-107`), which is the
  architecture's hard rule 1 held in practice.

## Not covered by this pass

Said plainly so the gaps are not mistaken for clean results:

- **IME / Chinese input.** `docs/IME_CHECKLIST.md` is a human acceptance list; a
  headless shot has no TSF. Untested here.
- **Real window behaviour**: tray menu, frameless drag/resize, maximize mirror race,
  actual keyboard focus traversal. No desktop UI automation was used, so nothing in
  `09`'s second half and nothing about focus in `10` was observed on a live window —
  only on the software renderer's offscreen buffer. *(The file-association door has
  since been driven: the shell command is read out of `install/quire.iss:63` and a
  `--open` launch was run against a live session and inventory-confirmed with
  `EnumWindows` (`14`), and the import arms of `34`/`35`/`36` all go through that
  door's code. What is still not observed is a real double-click in Explorer — no
  `SHOpenWithList`, no association registered on this machine, no pixel of the
  resulting window.)*
- **LAN sync** beyond the settings panel's seeded scenes (which `04` shows were not
  on screen). *(The protocol itself has since been driven over real sockets —
  that is `21`; `28` adds the read-only share on 5877, and what is still unread is
  the two-peers-in-a-room merge.)*
- **Anything animated**: `07` means every `animate` property in an interaction shot
  is unread, so this pass cannot say whether the tweens *look* right, only that four
  of them interpolate (`08`).
- **The editor's own keystrokes.** `17`–`41` reach the same data through MCP,
  through `--open`, or through `--scene`, which applies the app's own
  `Command::SetBlockType` / `ReplaceText` calls (`controller.rs:10611-10640`) and its own
  attachment imports (`:10552-10605`) but is not the same entry point. So a table, a database,
  a picture and a file have all been built *inside* the app (`38`, `39`, `40`) without ever
  clicking the ＋ line: what remains unexercised is the pointer and keyboard path into those
  commands, the file dialog behind `pick_attachment`, and any block type reachable only from
  the editor UI.
- **A second machine.** Every transport arm in `37`, and every share and sync arm in
  `21`/`28`/`29`, speaks to `127.0.0.1` from the same box. The `0.0.0.0` binds and the
  firewall rules are read off the source and the policy store, never crossed by a real
  remote peer, so "any LAN peer can do this" is an inference from three measured facts,
  not a handshake from another host.
- **Body size through the MCP tool surface.** `37` measures what a *declared* length
  costs; a `create_note` with a 10 MB `body` argument was never driven, so nothing here
  says where a real payload limit is, if one exists.
- **The settings dialog's buttons.** `25`'s 立即备份 and `28`'s 共享 switch are both
  reported from their handlers (`state.rs:3310-3317`, `controller.rs:5602-5606`),
  because pressing a toggle needs desktop UI automation, which this pass does not
  do. What those two handlers fail to do — start or stop a listener, back anything
  up — is read off the two statements at `controller.rs:5603-5604`, not driven.
- **`--pull`, and any remote host.** Both doors have now been driven — `--share` is
  `28`, `--pull` is `30`, `31` and `32` — but every pull arm fed the app a file
  captured from a live share and re-served by `pullserver.js` on `127.0.0.1:5899`.
  Two Quire sessions on two machines talking at the same time has never run here, and
  `29` reads the firewall policy store without completing a handshake from another
  machine, so "a peer on the segment can connect" remains an inference about rules,
  not an observation of a session.
- **The two desktop export doors.** `38` compares four doors that all reach
  `export_page`, but only the two network ones were driven: the clipboard action
  (`controller.rs:8415` `copy_current_page_markdown`) and the *Export Markdown* file dialog
  (`:8551` `export_current_page`, whose `rfd::FileDialog` is at `:8573-8576`)
  are cited from their call sites — both pass a real database reader where the share
  passes `&|_| None` — and never pressed, because one needs a clipboard probe and the
  other a `rfd::FileDialog`. The consequence drawn in `38` (a shared library loses the
  database while the desktop copy keeps it) rests on reading those two functions, not
  on running them.
- **Which copy of a big picture leaves.** `41`'s fetches returned the file the store
  holds, byte for byte, but all three fixtures are 640×400 with `thumb = ''`, so
  `display_path` and `stored_path` (`controller.rs:707-708`) are the same file and the arms
  cannot tell them apart. A picture over `MAX_EDGE = 1280` — the one case where the peer might
  get the downscaled `.cache.png` instead of the user's original — was never produced.
- **A dead link being clicked.** `40` establishes that no code resolves
  `quire://attachment/…` (only `quire://block/` and `quire://page/` have arms), but the link was
  never activated; that is a pointer in the editor, and this pass does not drive desktop UI.
- **The ⋮⋮ "Move to" menu, pressed.** `42`'s move arrives through the merge door
  (`state.rs:16584`), which pushes the same `Change::BlockMovedToPage` the menu reaches by way of
  `controller.rs:5172` → `state.rs:2719` → `command.rs:2365`. Those three call sites are read,
  not pressed, and the five blocks moved in `sdrsub.sh` are flat markdown paragraphs — the
  per-subtree loop at `command.rs:2359-2365` is a code reading with a five-block measurement
  next to it, not a deep-tree test. What `42` also does not show is the pixel: no screenshot of
  a drifted hit in the panel, and no observation of what `search-open-hit` visibly does when the
  block is not on the page it opens.
- **`task_lists.name` as a search source.** `sdcov.sh` finds the table in `sqlite_master` and
  zero rows in every library this pass built, so that source is unmeasured; what can be said of
  it is the call-site inventory in `42` A. The organizer's own two doors ("搜索笔记", "搜索任务")
  and the view's "搜索此视图" were likewise never driven — they are why the coverage boundary is
  a boundary and not a hole.
- **A release binary.** Every functional arm ran the debug exe. The claim in `25`
  that a shipped build has nowhere to print its one warning line is an inference
  from `src/main.rs:6`, not a measurement; this pass built no release artifact.

## Re-running the evidence

Analysis scripts live in `.scratch/issue-test/` (gitignored, written for this pass):
`bmpdiff.js` (24-bit BMP diff + bbox), `bmpprobe.js` (`col|row|at|box|uniq` pixel
scans), `cropdiff.js`, `sweepaudit.js`, `sweepcheck.js`, `listcheck.js`,
`contrast.js`. Captures used in the records are under `.scratch/issue-sweep-20261017/`
(186 PNGs) and `.scratch/issue-test/scr/`. Each record's command block is
self-contained against `target-shot/debug/quire-shot.exe`.

One write outside `.scratch/` was made, because the gate would otherwise have been
judging an 11-day-old renderer: `target/debug/quire-shot.exe` was overwritten with
the current `target-shot` build (`01`). No file under `src/`, `ui/`, `benchmarks/`,
`install/`, `justfile` or `Cargo.toml` was modified, and no version was bumped.

The functional rigs (`17`–`39`) live in `.scratch/issue-test/ftest/` and run
against a *copy* of the tree, `sandbox-20261017/quire-desktop`, whose debug exe is
the only binary any of them launches:

- `mcp-e2e.js` — the 10-tool battery; `bulk.js` / `mknotes.js` / `listcount.js` —
  writes and "what does the session think it can see";
- `q.js` (`node:sqlite`, one SQL statement against a copied library) and
  `dbprobe.js` (read-only schema/row peek);
- `flushclock{,2,3}.js`, `durab3.js` + `durab3.sh` — the `20` loss-window arms;
- `duecheck.js` — the `18` date arms; `syncprobe.js`, `syncpair.js` — the `21`
  arms, both PID-gated on 5878/5879;
- `memmode2.sh` (`L1` locked library, `L2` corrupted library) and `snapclock.sh` +
  `snappeek.sh` — the backup-timeline arms (`25`, `26`);
- `palettesearch.js` — parses the command rows and the `@keys` bindings out of
  `state.rs` and `AppWindow.slint`, printing the counts it found before judging
  anything (`27`);
- `lan.sh` / `lan2.sh` / `lan3.sh` with `lanprobe.js`, `lanprobe2.js`,
  `exportscan.js` and `mkorg.js` — the share arms (`28`): control vs `--share`, the
  persisted `lan.share` row, the route table, the template exclusion checked against
  the database, and the 8-client flood. Each one refuses to speak to 5877 unless the
  listener pid belongs to the session it just started;
- `fwp.ps1` / `fwp2.ps1` — read-only enumeration of the firewall policy store for
  `29`: `Get-NetFirewallRule` paired with its application, port and address
  filters, then `Test-Path` on every distinct program path;
- `pullserver.js` — the mock peer for `--pull`, one process per endpoint shape
  (`ok stall boom html nohdr dribble hold slow big`), and `pull.sh` / `pull-arms.log`
  — the ten arms of `30`, each sampling the working set every 2 s, checking whether
  the session can still be asked to quit, and gating on its own pid. `pullquit.sh` +
  `winprobe.ps1` are the same arms with timestamps and a window inventory;
  `mcpdead.js` / `mcpwrite.js` ask the MCP server the same question while a pull is
  open; `pulldead.sh`, `pullwrite.sh`, `pullqueue.sh`, `pullabort.sh` cover the
  late-landing queue, the hard-kill, and the abort line on the next launch;
- `getfile.js` — one plain HTTP/1.0 GET to a file, used to capture the *real*
  `/api/export` bytes; `mkfid.js` + `pullfid.sh`, `mkdepth.js` + `pulldepth.sh`,
  `mkinject.js` + `pullinject.sh` and `mkorphan.js` + `pullorphan.sh` + `doorgap.js`
  are the fidelity arms of `31` and `32` (hostile titles, a five-deep chain, text
  outside every frame, a code block that quotes the frame format, and an orphan page
  the schema refuses to let exist). Each one aborts rather than reporting a clean
  number if its own capture is short of frames;
- `waitport.js` — the TCP probe that replaced the stale `mcp.json` check.

`37`'s rig is `mcpdrain.sh` + `mcpdrain.js`, six modes (`headers`, `sync`,
`authorder`, `race`, `park`, `deadline`), each starting its own bounded session and
each writing its output and its library copy to `ftest/mcpdrain-logs/<mode>/` — because
the case directory is rebuilt on every run, and four of the six tables in that record
first existed only in terminal scrollback. That paragraph cost a re-run: the preserved
copy took only `quire.db`, and a library whose rows are still in the `-wal` reads back
empty, so three `notes=0` rows in the first transcription were the harness, not the
server. The rig now copies `snap/*` (`.db`, `-wal`, `-shm`) and the numbers in `37` are
transcribed from the kept logs.

The `33`–`42` arms added:

- `orgprobe.sh` + `orgprobe.js`, then `org2.sh` + `org2.js` — the organizer battery
  (`33`): one throwaway `--db` per leg, every call sequential and 400 ms apart so the
  250 ms `tools/call` pump (`24`) cannot be mistaken for tool behaviour, and the
  answer read back with `select id,quote(tags) …` from the snapshot after a graceful
  `--quit`;
- `cliprobe.sh` + `cliprobe2.sh` + `cliprobe-fx/` — the `--open` door (`34`): the
  nine encodings and shapes (UTF-8, GBK, UTF-16 with BOM, a png, a directory, a
  missing path, an empty file, a bare positional, no flag at all), each with the
  page/block census read from the file so a refusal and a partial write cannot be
  confused;
- `bigprobe.sh` + `relaunch.sh` — the cost curve (`35`): one-second sampling of
  `Responding`, `MainWindowHandle`, the listener set and the working set while a
  100,000-line import runs, then the same library relaunched with **no** `--open` to
  measure what the import leaves behind;
- `flagprobe.sh` + `flagprobe4.sh` — the flag parser (`36`): fifteen arms, each
  printing its own argv, its session/loop timestamps, `mylisteners` gated on its own
  pid, the console bytes, whether the asked-for exit actually happened, the `--quit`
  rc and the settings read-back, including the seeded-`lan.share` `revert`/`retnothing`
  pair that shows a discarded port falls through to the persisted default;
- `secondwin.sh` + `winlist.ps1` — the file-association door against a live session
  (`14`, `36`): `winlist.ps1` enumerates that pid's top-level windows with
  `EnumWindows` and prints class, title, visibility, extended style and the title as
  UTF-16 hex, which is how the modal box was identified as `class=#32770` captioned
  「Quire 已在运行」 without a screenshot or a click. Both scripts kill only by the
  Windows pid read back from `quire.log` or `Win32_Process`, never from `$!`;
- `mdtable.sh` + `mkpipe.js` — the table legs of `39`: `--scene table` builds a real
  grid through the app's own `SetBlockType`, the share is asked for `<page>.md` with
  `getfile.js`, the same bytes are handed back through `--open`, and the read-back is
  a block-kind census. `mkpipe.js` seeds two hostile cells (`A|B`, a cell holding a
  newline) by `update blocks set text = ?`, printing the cells before and after so a
  zero-row answer cannot be read as "no such cell";
- `mdpipe.sh` — the re-export leg: the same library exported, imported, then exported
  again, which is what separates "the escape is wrong" from "the two halves disagree";
- `mddb.sh` + `mdbase.sh` — the database legs of `38`. `mddb.sh` runs
  `--scene database-table`, then the page door, `/api/export` and an `--open` of the
  captured file, and reads the row counts out of `databases`/`db_properties`/
  `db_records`/`db_views`/`db_values`. `mdbase.sh` is its control on an untouched seed
  library, because a `pipe_lines=0` measured only against a page that has no tables
  proves nothing about the exporter;
- the schema these two rigs query was read from `pragma_table_info` after the first
  version guessed it wrong (`databases` has no `ord`, `db_views` stores `layout`, not
  `kind`), and `q.js` now falls back to `setReadBigInts` because the `ord` columns are
  full i64 order keys and `node:sqlite` throws `RangeError` on any value past 2^53;
- `attprobe.sh` + `attpath.sh` — the media legs of `40`. The first runs
  `--scene image`, `--scene file` and `--scene image-half` as three sessions on one
  throwaway library, lists the `attachments/` folder against the `attachments` rows, exports
  the page, re-serves the captured frame through `pullserver.js` for a `--pull`, imports the
  same bytes with `--open`, and reads both destinations back. The second is the URL battery:
  ten spellings of one page id, each printing the body's **md5** next to its status, because
  ten `200`s alone cannot tell "serves the page" from "serves an error page the same length".
  Its positive controls are in the same table — a template id, `0`, `-1`, `102abc`, `.`;
- `attsync.sh` + `getbin.js` — `41`'s byte arms. `getbin.js` is a binary-safe GET (Buffers,
  header cut in bytes, md5 of the body printed): `getfile.js` does `buf += d`, which
  utf-8-decodes a PNG on the way in and destroyed the first attempt at the comparison that
  matters. The rig copies `40`'s library *and* its `attachments/` folder into a case directory,
  gates 5878 on its own session pid, fetches `/sync/info`, `/sync/snapshot`, then each id,
  compares md5 against the source file, tries six malformed ids, and repeats the 1.8 MB fetch
  ten times for a cost figure;
- both media rigs read `marks` as the **table** it is (`migrations.rs:142-149`, columns
  `block,start,end,kind,url`). The first query — `select marks from blocks` — answered
  `Error: no such column: marks`, and the resulting "no row contains the pointer" was almost
  recorded as the link being dropped entirely;
- `sdrmove.sh` + `sdrsub.sh` — the mirror arms (`42`). Each seeds two pages through the app's
  own `--open` door, starts one session on a throwaway `--db`, gates UDP 5879 on that session's
  pid before `announce.js` sends a single discovery datagram, GETs the snapshot the app itself
  emitted, and POSTs it back — first **verbatim** (the round that builds the peer's shadow),
  then with `page` re-keyed for one block or for the whole page. `mksnap.js` edits that body as
  *text*, never through `JSON.parse`→`stringify`, because `ord` is a full i64 order key and a JS
  number rounds it; `moveall` is bounded to the `"blocks"` array by a string-aware depth walk
  (`SRecord` serialises a `page` field too, `services/sync/model.rs:212-215`), prints the ids it
  re-filed, and refuses the body unless `EXPECT` equals the block count read from the database.
  The verdict is two SQL statements run against a copy after `--quit`: `blocks` joined to
  `search_blocks` on `rowid` where `page <> page_id` (drift), and blocks holding text whose id
  is absent from `search_blocks` (loss). `sdrift.sh` is the earlier version, kept because its
  first two arms are what showed the merge needs a common ancestor;
- `sdcov.sh` + `buildmatch.js` — the coverage boundary of `42`, offline against five libraries
  that already exist on disk. For each term: which table really holds it, whether it is in the
  mirror's two sources at all, whether it is in the index by `LIKE`, and what the app's own
  `MATCH` returns. `buildmatch.js` transcribes `search_index.rs:26-96`, so the needle reaching
  `MATCH` is the expression the app would build — `"假 标 题"*` for CJK, not the raw run of
  Han, which would have matched nothing and looked like a second defect.

The sandbox copy is disposable and holds no user data; every case directory under
`ftest/` was created by these scripts with its own `--db`.

