# 32 · a page that quotes the frame format forges pages on the machine that imports it

**Severity: medium** — nothing here is a malformed file, and that is the point. The
source library holds one ordinary page whose body documents the share protocol in a
fenced code block, which is exactly where a person writes such a line. Its
*single-page* Markdown is correct, and its *whole-workspace* frame is not: the
fence is invisible to the splitter, so the importer creates a page the source never
had, breaks the code block into an empty one, and throws away the sentence that
came after the forged `<<<QUIRE END>>>`.

## Evidence

`mkinject.js` (offline SQL, no session holding the library) seeds page 200
`Frame format notes` with three blocks — a paragraph, a `code` block whose text is
the frame format itself, and a paragraph carrying a marker sentence:

```
blocks columns: id page* kind* text checked color bg page_ref folded attachment img_percent columns lang db_ref sync_ref
seeded: [{"id":9000000001,"page":200,"kind":"paragraph","t":"The frame format puts one marker line per pa"},
         {"id":9000000002,"page":200,"kind":"code",     "t":"<<<QUIRE PAGE: injected>>>\nthe body under th"},
         {"id":9000000003,"page":200,"kind":"paragraph","t":"TAIL-LINE: this sentence belongs to Frame fo"}]
[{"real_pages":15,"blocks_on_200":3}]
```

The `block_children` rows are not decoration: `select * from blocks` returns three
rows whether or not the child rows exist, but the loader enumerates blocks through
an INNER JOIN on `block_children` (`core/storage/repository.rs:275-278`), so the
first version of this rig — which wrote only `blocks` — served the page as an empty
frame and proved nothing. The rig now asserts `blocks_on_200: 3` before it shares.

**The two doors disagree about the same page.** `pullinject.sh` captures both from
one live `--share` session, PID-gated on 5877:

```
/api/page/200.md (single-page door, no framing) — 214 B:
    # Frame format notes

    The frame format puts one marker line per page:

    ```
    <<<QUIRE PAGE: injected>>>
    the body under the injected marker
    <<<QUIRE END>>>
    ```

    TAIL-LINE: this sentence belongs to Frame format notes.

export: bytes=7695  marker lines=16  END lines=16   (the library has 15 real pages)
```

The single-page door is right: `export_service` fences the code block and the
marker lines are inert text. The frame door writes the same body **verbatim**
(`lan_server.rs:183`) after its own marker line, and here is the captured region,
line-numbered out of `pullinj-src/export.md`:

```
184  <<<QUIRE PAGE: Frame format notes>>>
185  The frame format puts one marker line per page:
186
187  ```                                   ← the fence opens
188  <<<QUIRE PAGE: injected>>>            ← inside the fence, and it is a frame boundary
189  the body under the injected marker
190  <<<QUIRE END>>>                       ← and a page terminator
191  ```                                   ← the fence closes, one page too late
192
193  TAIL-LINE: this sentence belongs to Frame format notes.
194  <<<QUIRE END>>>
```

`grep -c '^<<<QUIRE PAGE: '` is 16 while the library holds 15 non-template pages:
the extra frame line came out of a code block.

**What lands on the importing machine.** A clean library (19 pages) pulls exactly
those bytes:

```
console: quire: mcp server on 127.0.0.1:5880;quire: imported 16 pages from …;   ← 16, not 15
[{"pages_after":35,"named_injected":1}]

  133 'Frame format notes'   parent=null
  134 'injected'             parent=null      ← the forged page, as a real page

where did the three lines of page 200 go?
  [{"page":133,"kind":"paragraph","t":"'The frame format puts one marker line per page:'"},
   {"page":133,"kind":"code","t":"''"},                                                        ← the fence, now empty
   {"page":134,"kind":"paragraph","t":"'the body under the injected marker'"}]                 ← code, downgraded to prose
  the tail sentence, counted:
  [{"tail_rows":0}]
```

So one page of three blocks arrives as two pages and three blocks, of which:

- a page titled `injected` exists that never existed on the source;
- the source's `code` block arrives **empty**, its contents sitting in the forged
  page as a `paragraph` — the block kind is lost as well as the owner;
- `TAIL-LINE: this sentence belongs to Frame format notes` is gone. `unframe_workspace`
  only keeps a line while a frame is open (`lan_server.rs:215`), and the forged END
  at 190 closed the frame four lines early, so 191–193 are outside every page.

That is content loss in a read-only import, with no word in either console:
`imported 16 pages` is a true count of the frames and a false count of the pages.

## Cause

- `lan_server.rs:182-187` writes `<<<QUIRE PAGE: <title>>>`, then the Markdown body
  byte-for-byte, then `<<<QUIRE END>>>`. The **title** is sanitised
  (`:173-174`, `replace(['\n','>'], " ")`); the **body** has no escaping of any kind,
  so the format is not self-delimiting. `frame_workspace`'s own doc comment
  (`:150-151`) says *Titles are sanitized so they can never contain the boundary
  marker* — it is accurate about titles and silent about bodies.
- `lan_server.rs:200` `for line in body.lines()` — the splitter is line-oriented and
  knows nothing about Markdown fences. A `code` block is just lines by the time it
  reaches the frame (`export_service` writes ```` ``` ```` around it), so anything
  the user wrote inside one is parsed as protocol.
- `lan_server.rs:210-214` — the first END wins. A forged END ends the page, and the
  remainder is discarded at `:215` rather than re-attached to the page it came from.
- The importer has no opinion about the titles it is handed: `controller.rs:8250-8254`
  trims, substitutes `已导入` for an empty title, and `:8255` `create_page(None)`
  makes each one a root page. A peer therefore controls the *number* and the *names*
  of the pages that appear in the destination's sidebar.

## What this is not

- **Not a new way in.** Record 30 already established that `--pull` reads whatever a
  URL answers, with no deadline and no size cap, and record 28 that the share socket
  authenticates nobody. A hostile peer able to serve arbitrary bytes can already
  write arbitrary *content* into the importer; this record adds that it can choose
  the page boundaries and titles, and that the loss runs even with nobody hostile in
  the picture.
- **Not a template leak.** `template: false` is hard-coded at `controller.rs:8269`
  and `frame_workspace` filters templates at `:169`; a peer cannot import a template.
  `SPEC §三十八`'s exclusion holds on this path.
- **Not an orphan-page hole in the tree walk.** I tried to seed a page whose `parent`
  points at a non-existent id, to see whether the recursive walk at
  `lan_server.rs:160-189` loses a page that `/api/pages` (`:92`) lists. The insert
  was refused: `pages.parent` is `INTEGER REFERENCES pages(id) ON DELETE CASCADE` and
  `node:sqlite` enforces it (`Error: FOREIGN KEY constraint failed`, and the app sets
  the same pragma at `core/storage/database.rs:69,85`). So a committed library cannot
  hold an orphan, `ON DELETE CASCADE` removes the children with the parent, and the
  walk cannot go blind that way. `repository.rs:440` does turn on
  `defer_foreign_keys` inside a transaction, which is where a future pass would look
  if it wanted to test the state at commit time rather than at insert time — this
  pass did not.

## Not measured

- **A malicious peer deliberately forging frames.** The bytes here came from a real
  `--share` session of a real library containing a real code block. A hand-written
  file with `<<<QUIRE PAGE: Weekly Review>>>` repeated, aimed at a destination that
  already has that page, would be the attack shape; this pass did not build it,
  because it needs no new code to predict (the splitter has no id and no dedupe).
- **What the user sees.** `controller.rs:8286` sets `db_notice` to 「已从 <url> 导入 N
  个页面」, so the on-screen count is the forged one (16). No desktop UI automation was
  used, so the notice is read off the format string, not photographed.
- **The next round trip.** Re-sharing the destination library would emit
  `<<<QUIRE PAGE: injected>>>` for a page whose title carries no marker characters,
  so the *forged page* becomes an ordinary page; the `Frame format notes` page,
  though, now holds an empty code block and has lost its tail sentence permanently.
  Not driven — it would need a second capture arm.

## Candidate fixes

- **Escape the body, or stop using a line format.** The cheapest is to neutralise a
  body line that begins with the marker: write `>` as `\>` (or a zero-width space)
  when framing, undo it when splitting. The honest version is a format that carries
  length instead of markers — `Content-Length` per frame, or one JSON object per
  line — because a text protocol whose delimiters can appear in its payload will
  keep producing bugs of exactly this shape. `frame_workspace` and
  `unframe_workspace` are ~40 lines each and have one caller apiece.
- **Track the fence in the splitter**, if the format stays: `lan_server.rs:200`
  could hold a `in_fence` flag toggled on ```` ^``` ````, and treat marker lines
  inside it as content. That fixes the measured case and leaves the general hole
  open, so it is a patch, not an answer.
- **Re-attach rather than discard.** A line seen with no frame open
  (`lan_server.rs:215`) could go to a `preamble`/`trailing` bucket that the importer
  puts in a page and the console counts, which is also record 31's fix for text
  outside frames — the same `else` branch handles both.
- **Refuse, with a number.** `/api/pages` says how many pages the source has;
  `--pull` could compare that count (one extra GET) with the frames it unframed and
  say `the peer listed 15 pages and framed 16` before writing anything, instead of
  importing the difference.
- Whatever is chosen, keep what works: the single-page door's fence handling
  (`/api/page/200.md` was right), the template filter at both `:169` and `:127`, and
  the 405/400 guards `28` measured. `mkinject.js` + `pullinject.sh` are the gate for
  any change here — the rig asserts `blocks_on_200: 3`, 16 marker lines for 15
  pages, and `tail_rows: 0`, so a real fix must make those three numbers read
  3 / 15 / 1.
