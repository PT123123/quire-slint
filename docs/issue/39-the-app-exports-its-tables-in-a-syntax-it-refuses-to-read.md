# 39 · the app exports its tables in a syntax it refuses to read, so one round trip turns a grid into three paragraphs and a rule line

**Severity: medium** — `--scene table` makes the app build its own 3×2 grid,
`GET /api/page/102.md` writes it out as GitHub-Markdown, and `--open` of that exact
file puts it back as **three literal paragraphs and zero table blocks**. Measured:
`| North | South | East |` / `| --- | --- | --- |` / `| West | Up |  |` → three rows in
`blocks` with `kind='paragraph'`, `select count(*) … where kind in ('table','table_cell')`
= **0**. The second half is worse than a lost grid: the exporter escapes a cell's pipe as
`\|` (`export_service.rs:248`) *because* a bare pipe would end the column, and the
importer's inline pass deletes that backslash (`import_service.rs:404-409`), so
`A|B` leaves as `A\|B` and comes back as a paragraph whose text is `| A|B | …` — a line
that now *looks like* a four-column table to any GFM reader, including the next tool the
user points at the file.

## Evidence

`bash mdtable.sh` (legs A–D) and `bash mdpipe.sh` (legs F–H) —
`ftest/mdtable.sh`, `ftest/mdpipe.sh`, `ftest/mkpipe.js`. Each leg its own throwaway
`--db`, its own 5877 PID gate, library read back from a `db + wal + shm` copy, pid from
`quire.log` and never `$!`.

### A. The app builds a table, and it persists

`--scene table` (`main.rs:123` → `controller::apply_scene`, `controller.rs:10611-10640`)
turns the first non-empty paragraph on the open page into a grid and fills the cells with
`North South East West Up`. This is the app's own write path, so the shape is what a
user's would be:

```
kinds:  … todo=6 table_cell=6 quote=3 … toc=1 table=1 embed=1
grid rows: 7 (table, table_cell ×6)
table:  {"id":1000000012,"page":102,"columns":3,"text":""}
cells:  1000000145 North · 46 South · 47 East · 48 West · 49 Up · 50 (empty)
```

`columns = 3` with 6 cells is `TABLE_DEFAULT_COLUMNS = 3` and `TABLE_DEFAULT_ROWS = 2`
(`command.rs:503-504`, built as `cols * TABLE_DEFAULT_ROWS` at `:1577-1578`). After
`--quit rc=0 survived=no ended_lines=1`, the same two queries return the same block, the
same columns and `North / South / East / West / Up / ` — the grid is in the file.

### B. What the export door writes

`GET /api/page/102.md` → `HTTP/1.0 200 OK`, body 1449 B. Diffed against the untouched
control (`mdbase.sh`, same seed page with no scene, 1551 B):

```
3c3,5
< A quiet home for thinking. This workspace collects notes, plans, …
---
> | North | South | East |
> | --- | --- | --- |
> | West | Up |  |
```

That is a correct GFM table, and the `---` line is what `render_table` writes unconditionally
after row 0 (`export_service.rs:243-258`): **the exporter promotes the grid's first row
to a header row**, which the app itself has no concept of — `BlockKind::Table` is a plain
cell grid (`state.rs:6416-6437` reads `children` and `columns`, nothing about a header).
The line the scene consumed is the paragraph at A, so nothing else moved.

### C. What the import door reads back

`--open` of that same file into a fresh library:

```
kinds:      bullet=60 paragraph=39 … heading_1=1 embed=1     (no table, no table_cell)
grid rows:  {"n":0}
pipe rows:  {"n":3}
what the three lines became:
  {"kind":"paragraph","text":"| North | South | East |"}
  {"kind":"paragraph","text":"| --- | --- | --- |"}
  {"kind":"paragraph","text":"| West | Up |  |"}
```

Three separate blocks, in the page's display order, with a real blank line between each —
and the imported page (`heading_1` "Getting Started", 27 blocks) is otherwise a faithful
copy of the source page. So this is not a failed import: every line arrived, and the
*structure* is what did not.

### D. Control

The same five words in a file with no pipes (`plain.md`: `# Revenue table`, one line of
words, one sentence) arrive as `heading_1` + 2 paragraphs, `grid rows: 0`,
`pipe rows: 0`. The door works; it is the table syntax it does not read.

### E. The escape, out and back (`mdpipe.sh`)

Leg F writes `A|B` into cell 145 and `top\nbottom` into cell 146 of leg A's library
(`mkpipe.js`, text-only update, no session holding the file), then exports:

```
exported line 3:  | A\|B | top bottom | East |
                  | --- | --- | --- |
                  | West | Up |  |
```

Both documented cell rules fire: `|` → `\|` and newline → space
(`export_service.rs:244-248`, "a pipe would end the column, a newline the row: neither is
representable inside a cell, so both become text").

Leg G imports that file into a fresh library:

```
kinds:      … paragraph=39 …            (no table, no table_cell)
grid rows:  0    pipe rows: 3    backslash rows: 0
text:  "| A|B | top bottom | East |" · "| --- | --- | --- |" · "| West | Up |  |"
```

`backslash rows: 0` is the finding: the `\|` the exporter wrote to keep the row intact is
**removed** by the importer's escape pass, which by design drops the backslash and keeps
the punctuation (`import_service.rs:404-409`). Each side is right about its own format;
joined, they turn a protected pipe into a raw column separator sitting in a paragraph.

Leg H re-exports the page leg G made (`GET /api/page/119.md`, 1466 B):

```
5:| A|B | top bottom | East |
7:| --- | --- | --- |
9:| West | Up |  |
```

The paragraphs are **not** re-escaped — the escape lives in `render_table`'s cell closure,
and these lines are paragraphs — and they now sit three blank-line-separated lines apart
instead of three consecutive lines. The file has stopped being a table and started being
three lines that a Markdown reader may still try to read as one.

## Cause

One asymmetry and one collision.

- **The two doors were written to different grammars, and the source says so.**
  `import_service.rs:7-10`: *"What it does not recognize — **tables**, images, setext
  headings, footnotes — is kept as literal paragraph text, which is what makes
  `export_page(import(x))` semantically stable: text we cannot represent is never thrown
  away."* True as written for text the *user* typed. False as a round-trip guarantee,
  because `export_page` **invents** syntax the user never typed — the `| --- | --- |`
  rule row in particular — and that invented line is not "text we cannot represent", it
  is the exporter's own punctuation arriving as content. The claim in the comment is a
  one-direction statement (`import(export)` of arbitrary text) sitting next to a door
  that is used in the other direction (`export` then `--open`).
- **The escaping halves do not meet.** `render_table`/`render_database` escape pipes
  (`:248`, `:185`); the block-type dispatch at `:147-155` sends a table out as GFM; and
  the importer never enters a table branch at all, so the escaped form is handed to the
  inline pass, whose escape rule (`:404-409`) is written for prose. Two correct rules,
  one missing: nothing reads a `| … |` line as a grid.
- **No header concept to receive the header row.** `render_table`'s own doc comment says
  *"The first row is the header row — the same convention the block itself starts with,
  and the only shape a Markdown table has"* (`export_service.rs:231-233`), but the store it
  is describing has no header: the `blocks` table's columns are
  `id,page,kind,text,checked,color,bg,page_ref,folded,attachment,img_percent,columns,lang,db_ref,sync_ref`
  (read out of `sqlite_master`), and the grid reader takes only `columns` and the
  `TableCell` children in display order (`state.rs:6416-6437`). So even a hypothetical
  GFM-reading importer would have to invent the mapping (header ⇒ row 0, no styling): the
  loss is partly representational, not only parsing.
- **The newline→space rule is one-way by construction** (`:248`): a cell that held two
  lines arrives as one, and nothing in the format marks that it was ever two.

## What this is not

- **Not a crash or a refusal.** Every leg ends with `--quit rc=0`, `survived=no`,
  `session ended` in the log, and the library holds what the census says it holds. The
  import reports success because it *is* a success — of the wrong grammar.
- **Not a lost document.** `pipe rows: 3` means the user's five words survived the trip;
  the grid relation between them did not. Nothing is deleted, and `34`/`35` cover the
  cases where the door refuses outright.
- **Not a share-door bug.** `38` is about the share dropping a *database* block; a
  **table** block exports correctly here (B) — `render_table` needs only blocks, which is
  why it survives `export_page`'s empty database reader.
- **Not measured in another Markdown reader.** "a GFM reader will show four columns" is
  the format's documented rule about an unescaped pipe meeting `render_table`'s own comment
  at `:246-247` ("a pipe would end the column"), not an observation: no other tool was
  pointed at these files.
- **Not a rendering claim.** What the grid looks like on screen is not measured here;
  `08`/`12` and the `table`/`table-edit`/`table-marks` scenes own that question.

## Candidate fixes

- **Teach the importer the table grammar it is paired with.** A `| … |` line followed by
  a `| --- | … |` line is unambiguous, and the app's shape is known: one
  `BlockKind::Table` with `columns = <cell count>` plus `cols × rows` `TableCell`
  children in display order (`command.rs:1577-1578` already builds exactly that from the
  menu). Ragged rows should pad rather than fail, the way `render_database` does it
  (`export_service.rs:177-179` states the rule, `:191-192` implements it with
  `row.get(at)…unwrap_or_default()`). This is the fix that makes the docstring in C true
  instead of one-directional.
- **If tables stay un-read, do not export them as one-way syntax.** Either export a table
  as something the importer also recognises (a fenced block, or a `|`-free per-cell list
  like `- North`), or mark the invented line as invented: an HTML comment the importer
  drops is enough to stop `| --- | --- | --- |` from arriving as a sentence.
- **Make the escape rule honest about who undoes it.** If the importer will strip
  `\|` in prose (it must — that is what `\*` means), then the exporter's pipe escape is
  only meaningful to a reader that parses tables. Say so in `:244-248`'s comment, or keep
  the cell text raw and quote the whole cell instead — a format both doors agree on is
  worth one comment line.
- **Round-trip gate.** Legs A–H are already the assertions; the fix version is checkable
  in one line each: after `--open` of an exported page that held a table,
  `select count(*) from blocks where kind='table'` > 0; the `\|` in the exported file is
  either absent (no escaping needed) or arrives as a `|` **inside a cell**, not a
  paragraph; and `| --- |` never appears as a block's `text`.
- **Cheap consistency win:** if `render_table` designates row 0 as a header, give the grid
  a header flag on the block (a `columns`-shaped column already exists) so the format's
  extra information has somewhere to land on the way back in.

## Not measured, said plainly

- **`--scene table` is the only table the app was made with.** It is the same
  `Command::SetBlockType { kind: Table }` the ＋ menu and "Turn into" use
  (`controller.rs:10623`), but a user-made 4×7 grid with marks inside cells was not
  built, so nothing here says how `**bold**` inside a cell fares — `render_inline` is
  called on cell text (`export_service.rs:245`) and the importer does know `**bold**`, so
  that particular pair probably round-trips. Probably is not measured.
- **Leg F writes `blocks.text` directly.** It changes only the two cells' text — no marks,
  no `search_blocks` row — and the app re-reads the row and exported it (B/E), so the
  shape it tested is the shape the app wrote. It is not the same as typing `A|B` into the
  cell editor, and a cell whose text carries marks would need the FTS row too.
- **The 3×2 default is the only size measured.** A single-cell or 1-column table hits the
  same two code paths, and `cols == 0 || cells.is_empty()` at `:240-242` returns an empty
  string for a degenerate one, which is `38`'s blank-line story again rather than this
  record's.
- **Nothing crossed a network here except the read-only share**, which is how the export
  bytes were captured; `--pull`, the sync snapshot and a second device are `21`/`30`–`32`,
  and 5877 was gated on this session's pid the whole time.
- **No pixel claim anywhere.**

## How this harness got it wrong

1. **A `sed` that cannot see two matches on one line.** Leg F's cell ids came out of a
   JSON array printed on a single line, and `sed -n 's/.*"block":\([0-9]*\).*/\1/p'` is
   greedy: it yields the *last* match once, so `P1`/`P2` were empty and the leg aborted
   with `ABORT: no cell ids to seed` — the honest failure mode, but only because the
   guard was there. Fixed with `tr ',' '\n'` before the `sed`. Any rig that reads `q.js`
   output with a greedy `sed` has the same hole.
2. **The first version of leg G read the backslash as a product bug.** The read-back
   prints JSON, and `JSON.stringify` of a text containing `\` shows `\\`; `0` rows from
   `instr(text,'\')` was therefore checked twice — once with the SQL, once by printing the
   three rows and seeing `| A|B |` with no backslash — before it became E. Without the
   second look this record would have claimed the importer *passes* `\|` through, which is
   the opposite defect.
3. **A page is not a control.** `mdtable` leg B's file (1449 B) is *smaller* than the seed
   page's (1551 B) because the scene consumes a paragraph, which looked like truncation
   until `mdbase.sh` exported the untouched page and made the diff show a substitution
   rather than a loss. `mdbase.sh` exists for that reason and is what every byte count in
   this record is measured against.
