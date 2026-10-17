# 31 · the LAN export frame is a one-way format: titles are rewritten, the page tree is dropped, and text outside a frame is discarded

**Severity: medium** — `--pull` does import pages, and `imported N pages` is an honest
number. What it is not is *your* pages. A five-deep tree arrives as six siblings whose
titles begin with literal `> ` characters; a title carrying a `>` or a newline arrives
with them replaced by spaces; and the prose a person writes above the first frame or
below the last one arrives nowhere at all. Nothing on either socket says a word about
this, and the suite's one socket-level round trip (`lan_server.rs:409-416`) frames a
library of exactly one root page, so it cannot see the tree it flattens.

## Evidence

Three rigs, each one session at a time, in `.scratch/issue-test/ftest/`:
`mkdepth.js` + `pulldepth.sh` (the depth chain and the out-of-frame prose), and
`mkfid.js` + `pullfid.sh` (the hostile titles). Every arm follows the same three
steps: seed a library through SQL with no session holding it, capture
`/api/export` **from a live `--share` session** so the bytes are the real wire
format rather than a hand-written guess, then `--pull` exactly those bytes into a
clean library and read the answer back out of *its* file.

The capture leg is gated: `pulldepth.sh` refuses to run unless the `0.0.0.0:5877`
listener pid is the session it just started, and refuses to continue unless the
captured file has ≥ 20 frame lines.

```
  5877 listener pid = 170680 (this session)
  HTTP/1.0 200 OK | headers=97 B | body=7858 B -> pulldep-src/export.md
  frames captured: 20 (the library holds 20 non-template pages)
```

**A. Depth 3 and depth 5 are the same line on the wire.** `mkdepth.js` inserted a
chain, `pages.parent` 210→211→212→213→214→215, one paragraph block each, and the
depth was computed in SQL rather than asserted from the insert:

```
seeded rows: 210 'D0 root' parent=null | 211 'D1 alpha' parent=210 | 212 'D2 beta' parent=211
             213 'D3 gamma' parent=212 | 214 'D4 delta' parent=213 | 215 'D5 epsilon' parent=214
depth of each: 210→0  211→1  212→2  213→3  214→4  215→5

the six chain frames, verbatim, with how many depth prefixes each carries:
    0 prefix | <<<QUIRE PAGE: D0 root>>>
    1 prefix | <<<QUIRE PAGE: > D1 alpha>>>
    2 prefix | <<<QUIRE PAGE: > > D2 beta>>>
    3 prefix | <<<QUIRE PAGE: > > > D3 gamma>>>
    3 prefix | <<<QUIRE PAGE: > > > D4 delta>>>     ← depth 4, written as depth 3
    3 prefix | <<<QUIRE PAGE: > > > D5 epsilon>>>   ← depth 5, written as depth 3
```

**B. After the pull, the tree is gone and the prefix is the title.** The same file
served back through `--pull` into a library that had never seen it (`pages` before:
19):

```
console: quire: mcp server on 127.0.0.1:5880;quire: imported 20 pages from …;

the six chain pages as they landed (quote() shows the real string):
  133 'D0 root'          parent=null
  134 '> D1 alpha'       parent=null
  135 '> > D2 beta'      parent=null
  136 '> > > D3 gamma'   parent=null
  137 '> > > D4 delta'   parent=null
  138 '> > > D5 epsilon' parent=null

bodies of the chain, as they landed:
  page 133 'the body of D0 root' … page 138 'the body of D5 epsilon'   ← all six, intact

any page in the destination with a non-null parent: n=7   ← the destination's own tree
```

The last line is the control, and it is what makes this a defect rather than a
limitation of the schema: the same file, in the same session, holds seven pages with
a real `parent`, so the column works and the importer chose not to use it. The
bodies are the second control — everything the frame carries inside a frame lands
exactly, which is why the two losses above are about *shape*, not about transport.

**C. Titles are rewritten, character by character.** `mkfid.js` put four hostile
titles into a library through SQL (`update pages set title=?`) and read them back:

```
source rows as stored:
  102 '<<<QUIRE PAGE: fid-marker>>>'  parent=null
  103 'Keys > Shortcuts fid-angle'    parent=102
  104 'fid-newline\nsecond line'      parent=102
  107 '> fid-looks-nested'            parent=106
```

What the live share wrote for those four, and what landed on the other side:

```
<<<QUIRE PAGE: <<<QUIRE PAGE: fid-marker   >>>     → 121 '<<<QUIRE PAGE: fid-marker'
<<<QUIRE PAGE: > Keys   Shortcuts fid-angle>>>     → 122 '> Keys   Shortcuts fid-angle'
<<<QUIRE PAGE: > fid-newline second line>>>        → 123 '> fid-newline second line'
<<<QUIRE PAGE: > >   fid-looks-nested>>>           → 126 '> >   fid-looks-nested'

how many landed titles carry the depth prefix: n=7   (of 14 imported pages)
every imported page: parent=null
pages after pull: 33  (19 + 14)
```

`>` became a space (`Keys > Shortcuts` → `Keys   Shortcuts`), `\n` became a space,
and the `> ` depth marker is welded onto the front of the title as literal text —
7 of the 14 imported pages now have titles that begin with `> ` in the destination
library. `写作与中文测试` round-tripped untouched, so this is not an encoding story.

The sanitiser does its one job: a page titled `<<<QUIRE PAGE: fid-marker>>>` cannot
become two frames, because `>>>` is replaced before the line is written, so the
line still parses as one page. But its *title* comes back as
`<<<QUIRE PAGE: fid-marker` — the prefix survives, and the trailing spaces are
trimmed off at `controller.rs:8250-8254`, so the two ends disagree about where the
marker ends.

**D. The same server does send the parent — on the other route.** `/api/pages`
writes a third column for exactly this purpose (`lan_server.rs:93-104`), and record
28 captured it verbatim: `103\tKeyboard Shortcuts\tunder #102`. `/api/export` drops
it to a `> ` prefix, and `unframe_workspace` returns `(title, markdown)` pairs with
no third field (`lan_server.rs:196`), so the pull client has nothing to restore.

**E. Text outside every frame is discarded, in both directions.**
`pulldep-body.md` is 15 lines: three lines of prose, then two correct frames, then
one trailing line — the shape of a hand-written index page or a README that leads
with a note about the export. The sentences that go in and out of frames were
checked by their own markers:

```
fixture: 451 bytes, frames=2, sentinel lines=2
console: quire: imported 2 pages from http://127.0.0.1:5899/api/export;
landed titles from the fixture: 119 'Frame One', 120 'Frame Two'
do the two outside-frame sentences exist anywhere in the library?
  select count(*) from blocks where text like '%SENTINEL%'  ->  n_from_blocks: 0
  select count(*) from pages  where title like '%SENTINEL%' ->  n_from_pages: 0
in-frame control: 119 heading_1 'Frame One' + paragraph 'the first framed body, FRAME-ONE-BODY'
                  120 heading_1 'Frame Two' + paragraph 'the second framed body, FRAME-TWO-BODY'
pages 21, blocks 149  (19 + 2, 145 + 4)
```

Four prose lines: three dropped before the first frame, one after the last
`<<<QUIRE END>>>`. Both in-frame bodies landed. The count printed, 2, is the number
of frames, so the line is true and uninformative: it does not say four lines went
nowhere.

## Cause

- `lan_server.rs:173-174` — `let prefix = "> ".repeat(depth.min(3))` then
  `title.replace(['\n', '>'], " ")`. `min(3)` is the collapse: depth 4 and depth 5
  are the same bytes. Replacing `>` is what rewrites the legitimate titles, and the
  prefix is written *into* the title string rather than alongside it.
- `lan_server.rs:188` recurses for the children, so the only carrier of the tree is
  that prefix, and `lan_server.rs:196` `unframe_workspace(body) -> Vec<(String,
  String)>` has no place to put it back.
- `controller.rs:8255` `state.create_page(None)` and `:8259` `parent: None` — the
  importer never asks for a parent, whatever the title says.
- `lan_server.rs:215` `else if title.is_some()` — a line is only kept while a frame
  is open, so the leading and trailing prose are not a special case anywhere in the
  code; they are simply outside the loop's `if`.
- `lan_server.rs:150-151` states the intent — *Titles are sanitized so they can
  never contain the boundary marker* — and the arms above confirm it for the
  terminator while showing that the sentence covers nothing else. There is no
  matching claim about the tree or about text between frames, and no SPEC or DECISIONS
  line that I found does: `grep -rn "QUIRE PAGE" docs/*.md` returns nothing, so the
  format is documented only in the help text at `lan_server.rs:76`.
- The tests that look like they cover this do not reach it. `lan_server.rs:409-416`
  `export_framing_round_trips_through_the_client` is the right *shape* — it starts a
  real server on a real port, GETs `/api/export`, and unframes the answer — but its
  fixture (`seeded_repo()`, `lan_server.rs:252-258`) is one page, `title: "Shared
  page"`, `parent: None`. `pages.len() == 1` is the whole assertion, so depth,
  `min(3)`, the prefix and the character replacement are all outside it. The other
  named round trip, `lan_client.rs:93-103` `unframe_round_trips_titles_and_bodies`,
  builds its input by hand, never calls `frame_workspace`, and asserts
  `pages[1].0 == ">> Child"` with the comment *'>' sanitized away at frame time* —
  pinning the splitter to a string the framer could not have produced, and calling a
  title that carries the depth prefix "correct".

## Not measured

- **Nothing about the two programs talking to each other at the same time.** Every
  arm here captures to a file and re-serves that file. A live peer on 5877 pulled by
  `--pull http://peer:5877/api/export` is the same bytes by `frame_workspace`'s
  construction, but this pass did not run two sessions concurrently, so "a real
  neighbour imports flat" is an inference from the file arms.
- **Whether a page title containing `\n` or `>` is reachable from the editor.** The
  four hostile titles were written with `update pages set title=?`. A user typing a
  `>` in a title is ordinary — that is how `Keys > Shortcuts` reads in the seeded
  library — but this pass drove no UI, so "the user can create a title the frame
  will rewrite" is a claim about the store column (`TEXT`), not about a keystroke.
- **What the sidebar shows after an import.** `import_lan_pages` ends with
  `open(...)` + `reproject_blocks()` (`controller.rs:8287-8288`), so the shell is
  told to redraw; the seven `> `-prefixed titles are read out of the file, not
  photographed.
- **The Markdown file doors** (`export_service`) are not implicated: `28` measured
  `/api/export` and `29`'s neighbours are about sockets. Record 32 is about the same
  framing and is where the *body* side of it bites.

## Candidate fixes

- **Put the parent on the wire, in the frame.** `/api/pages` already sends
  `under #<id>`; a `<<<QUIRE PAGE: <id> <title>>>` header (or a second field after
  the title) lets `unframe_workspace` return `(id, parent, title, markdown)` and
  `import_lan_pages` pass `Some(parent_id)` to `create_page`. Keep the `> ` prefix as
  *presentation* for a human reading the file, but stop using it as the only carrier.
- **Stop rewriting the title.** `>` and `\n` are only dangerous to the *terminator*.
  `frame_workspace` could keep the title byte-exact and put the escaping where the
  parser reads — or encode the title once (`base64`, or `\n` → `\n` and `>` → `&gt;`)
  and decode at `unframe_workspace`, so the round trip is a loop for every field.
- **Say what was dropped.** When `unframe_workspace` skips a line with no frame open,
  count it; when the count is non-zero, print it — `quire: imported 2 pages from … (4
  lines outside any frame were not imported)` — and put the same number in the
  `db_notice` at `controller.rs:8286`, which currently only appears when
  `imported > 0`.
- **Widen the round-trip fixture, not the test.** `export_framing_round_trips_through_the_client`
  already drives the socket, so the change belongs in `seeded_repo()`: a three-deep
  tree, a title carrying `>`, a title carrying `\n`, and a prose line before the first
  frame. Assert the *input* is recoverable rather than `len() == 1`. That one fixture
  fails on all three legs above today, and it is where `lan_server.rs:151`'s promise
  stops being a comment.
- **Cap the depth marker honestly.** If the prefix stays, `min(3)` should be
  `min(usize::MAX)` or the doc should say the tree is transmitted to depth 3 and no
  deeper, because a 4-deep workspace importing as a 3-deep one is a silent
  truncation of the user's structure.

## How this harness got it wrong

1. **The first run of arm A measured a broken file and reported it as a result.**
   I called `getfile.js … /api/export` with the leading slash; Git Bash rewrote the
   argument into a Windows path, the share answered its truthful 404 `not found`
   (9 bytes, `lan_server.rs:146`), and the arm went on to pull that 9-byte file and
   print `imported 0 pages`. That number is about my argument, not about the app.
   `pullfid.sh` and `pullinject.sh` had already hit this and are the reason
   `getfile.js` takes the path *without* its slash — a note in that file's own
   header comment — which I re-broke by writing a new caller. Fixed by using
   `api/export` and by adding the ≥ 20-frame self-proof above, which turns the same
   mistake into an abort.
2. **A `prepare()` throw is a clean failure and a lucky one.** `mkdepth.js` first
   version passed 13 values for 12 columns because the `ord` sub-query counts as a
   value; `node:sqlite` raised at compile time, before any row was written, so the
   library was untouched and the arm aborted at the seed step instead of measuring a
   half-seeded tree.
3. **`local` outside a function.** The gate I added for the 5877 listener used
   `local L77=…` at the top level of a `case` arm, which bash rejects — the helper
   functions in this rig are where `local` belongs. Two lines, fixed in place.
