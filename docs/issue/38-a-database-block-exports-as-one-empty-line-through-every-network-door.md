# 38 · a database block exports as one empty line through every network door, while the two doors on the desktop export it as a table

**Severity: high** — the app builds a real database (4 columns, 5 records, 20 values),
the library keeps every row of it, and the Markdown export answers for the whole thing
with **one blank line**. Measured: page 102 exports as 1551 bytes with no database and
1552 bytes with one, and the only difference in the file is an empty line at the end
(44 lines vs 43). `/api/export` — the frame `--pull` reads — carries 14 pages, names
that page and quotes its prose, and contains the strings `Name`, `Points`, `Done` and
`Due` **zero times**. The cause is one argument: the share and the frame go through
`export_page`, which hard-codes an empty database reader (`export_service.rs:37-39`),
while the desktop's 复制为 Markdown and 导出为 .md pass the real one
(`controller.rs:8428`, `:8567` → `state.rs:12998`), so the same page leaves the app two
different ways and the lossy one is the one a peer and a second device see.

## Evidence

`bash mddb.sh` — `ftest/mddb.sh`, three legs, each with its own throwaway `--db`, the
5877 PID gate enforced before the first byte, and the library read back from a
`db + wal + shm` copy. `bash mdbase.sh` is the control: the same seed page, no scene.

### A. `--scene database-table` builds one, and it persists

The scene is the app's own write path (`controller.rs:10393` → `seed_database_table`),
not a hand-made row, so the library holds what a user's would hold:

```
kinds:     bullet=53 paragraph=32 heading_2=24 divider=12 numbered=7 todo=6
           quote=3 heading_3=3 code=3 toc=1 embed=1 database=1
db block:  {"id":1000000145,"page":102,"kind":"database","db_ref":1,"text":""}
databases: {"id":1,"name":"Getting Started","template":""}
props:     1 Name/title · 2 Points/number · 3 Done/checkbox · 4 Due/date
records:   5   views: 1 (id 1, "Table", layout "table")   values: 20
pages:     19 (unchanged — the records are not pages: db_records.page is NULL for all 5)
```

After `--quit rc=0 survived=no ended_lines=1` the same query returns the same block, the
same 5 records and the same 20 values. Nothing here is broken: the database is in the
file, complete, and the app reads it back.

### B. What the share door writes for it

`GET /api/page/102.md`, `HTTP/1.0 200 OK`, body **1552 B**, `pipe_lines=0`. Diffed
against the control (`mdbase.sh`, same seed library, no scene, **1551 B**, 0 pipes):

```
43a44
>
```

That is the whole difference: one empty line. The database block is the last block on
the page (`order by c.ord desc limit 4` → `database`, then paragraph `Start typing, or
press…`, then the code block), and the file ends with the paragraph, a blank line, and
nothing else. `| Name | Points | Done | Due |`, the `| --- |` rule, and the five record
rows are **not in the bytes**.

The frame, from the same session:

```
GET /api/export → HTTP/1.0 200 OK, body 7451 B
  pages in the frame: 14 `<<<QUIRE PAGE` markers, 14 `<<<QUIRE END`
  "Getting Started"     ×1   (the page is there)
  "Start typing, or press" ×1 (and its prose is there)
  Name / Points / Done / Due ×0 (the database is not)
```

The two counts that matter are the last pair: this is not a page that failed to appear,
it is a page that appeared with its table missing.

### C. What the import door reads back

`--open` of that 1552-byte file into a fresh library:

```
kinds:     bullet=60 paragraph=37 heading_2=27 divider=13 numbered=10 todo=8
           quote=4 heading_3=4 code=4 toc=1 heading_1=1 embed=1
db rows:   {"block":0,"databases":0,"props":0,"records":0,"views":0}
pipe rows: 0
```

Correct behaviour, and that is the point: the importer cannot invent a database out of
nothing, so the loss in B is final. A round trip through the app's own two doors deletes
a table the user made, and the second device in a `--pull` pair never learns it existed —
it has no bytes to reconstruct it from.

### D. The doors are not equal, and the source says so

| door | call | database reader | measured / cited |
|---|---|---|---|
| LAN share page `GET /api/page/<id>.md` | `lan_server.rs:136` `export_page` | **none** | B: one empty line |
| LAN share frame `GET /api/export` | `lan_server.rs:181` `export_page` | **none** | B: `Points` ×0 |
| template export | `state.rs:3817` `export_page` | **none** | cited, not driven |
| 复制为 Markdown (palette) | `controller.rs:8415` → `export_page_full` at `:8423` | `db_markdown_table` (`:8428`) | cited, not driven |
| 导出为 .md | `controller.rs:8562` `export_page_full` | `db_markdown_table` (`:8567`) | cited, not driven |

`export_page` is three lines (`export_service.rs:37-39`):

```rust
pub fn export_page(blocks: &[Block]) -> String {
    export_page_with(blocks, &|_| None, &|_| None)
}
```

and its own doc comment names who gets what (`:75-76`): *"Callers that do not (a test,
**the LAN page writer**) go through `export_page` above and get the stored text, which is
what this layer did before there was a choice."* `render_database` (`:180-197`) is a
complete, correct GFM writer — header, rule, padded ragged rows, `|` and newline
escaping — and `database(block.id).map(render_database).unwrap_or_default()` at `:151-153`
is the only thing that calls it. With `&|_| None` it never runs.

## Cause

Not a bug in the renderer, a wiring decision that outlived the wiring:

- **ADR-0065's reader is threaded through the callers, not resolved in the exporter.**
  `export_page_with` takes `database: &dyn Fn(BlockId) -> Option<DatabaseTable>`
  (`:94`, `:112`) because "a database's content is not its blocks — it is records and
  values in six tables" (`:41-47`), and the exporter deliberately cannot reach a
  repository. Correct layering, one consequence nobody closed: `export_page` is the
  convenience wrapper that means "no reader", and `lan_server.rs` — added later, the only
  production caller left — calls the convenience wrapper.
- **The empty answer is invisible by construction.** `DatabaseTable::is_empty` (`:57-63`)
  says a table with nothing in it writes nothing, and `:157-165` iterates
  `rendered.lines()` — an empty string has no lines, so a database block contributes no
  marker, no comment, no `<!-- database -->`, and no error. `:83-86` states this is
  intended for a caller with no reader. It is a reasonable rule for a test helper and a
  silent one for a network endpoint.
- **Nothing distinguishes "no reader" from "empty table".** `None` and
  `Some(empty)` write the same bytes (`:83-84` says exactly that), so a peer cannot tell
  a database that was never filled from one the door refused to read.

## What this is not

- **Not a storage bug.** A is the whole counter-argument: 1 block + 1 database row +
  4 properties + 5 records + 1 view + 20 values, written by the app, kept through a
  graceful quit, read back by the next session.
- **Not a sync-protocol bug.** The 5878 snapshot serialises the six database tables —
  `merge.rs:562-587` compares `local.databases` as entities and flattens
  props/views/records/values for the three-way merge, and `engine.rs:403` reports
  `{dbs} databases` in its own summary. Two channels, two answers, and the Markdown one
  is the lossy one. (Read off those lines; the merge was not re-driven here.)
- **Not the record-pages case.** `db_records.page` is NULL for all five rows, so these
  records are not pages that could arrive separately through the frame's page list —
  which is why their absence is total rather than a lost view of existing content.
- **Not a rendering problem.** On screen the block shows a table (`state.rs:2337-2339`
  builds `row.table_cells` for `BlockKind::Table`, and the database rows come from
  `db_fill_row`); only the export path drops it.

## Candidate fixes

- **Give the share a reader.** `lan_server.rs:136` and `:181` already hold a `repo`
  (they call `repo.load()` at `:141`), so the page writer is a caller that *can* read
  databases. The fix is to stop calling `export_page` and call `export_page_full` with a
  closure over the loaded state — the same two arguments `controller.rs:8428` passes. The
  desktop's `db_markdown_table` lives in the app crate, not quire-core, so either the
  share needs its own reader in core or `lan_server` moves to the app crate; that is the
  one design question here, and `28`'s scope notes on where the share belongs decide it.
- **Say something when a block is refused rather than unread.** If `export_page` stays
  the no-reader door, then a `database` block reached through it should write a line the
  reader can see (`| (database not exported) |`, or nothing plus a count in the
  response), because today the file is *valid Markdown that quietly lies*.
- **Separate `None` from `Some(empty)`.** Make the closure return
  `Result<Option<DatabaseTable>, _>` or give `export_page_with` a
  `DatabaseRender::NotRequested` variant; then a caller that forgot to wire the reader is
  a different byte sequence from a database with no columns, and a test can assert on it.
- **Make `template_markdown` answer too** (`state.rs:3817`). A template whose page holds a
  database is exported with the same blind spot, and templates are the one thing this app
  *re-imports* by design, so the loss compounds there.
- **Gate it.** Extend `mddb.sh` with the two desktop doors once they are drivable, and
  pin these numbers as the failing assertions: `pipe_lines >= 1` for a page holding a
  database with records, `grep -c Points` ≥ 1 in `/api/export`, and `db rows` > 0 in the
  library that `--open`s the exported file. Today those three all measure 0, so the gate
  is three greps and a status line.

## Not measured, said plainly

- **The two desktop doors were read, not driven.** `复制为 Markdown` writes the clipboard
  and `导出为 .md` opens `rfd::FileDialog` (`controller.rs:8573`); both need a real
  window, which this pass does not automate. The claim "the same page exports two ways"
  rests on the two call sites in the table in D, which are quoted with their line
  numbers, and on B, which is measured. It is not restated as a pixel or a clipboard
  read.
- **The `--pull` consequence is an inference from a measured frame.** `31` and `32` prove
  the frame is the whole of what `--pull` imports and that it is line-oriented Markdown,
  and B proves the frame has no database bytes in it. No `--pull` arm was run against
  this library, so "the second device never learns it existed" follows from those three,
  not from a handshake.
- **One view, one layout.** `db_views` here has a single row, `layout = "table"`, and
  `definition` empty. Board/list/calendar/formula views (`controller.rs:10402-10427`) were
  not exported, so no claim is made about whether a non-table view renders, and ADR-0065's
  "current view" rule (`state.rs:13007`) is untested for the others.
- **Filtered and computed views.** `seed_database_filter`/`_formula` exist and were not
  used, so nothing here says whether a filter or a formula column survives.
- **5877 was gated, not crossed.** Every request came from `127.0.0.1` against this
  session's own pid; `21` owns the question of what a remote peer can reach.
- **No pixel claim.** The screen is described from the two source lines in *What this is
  not*, not measured.

## How this harness got it wrong

1. **`q.js` crashed on a legitimate row and I nearly read it as a product fault.**
   `select ... from db_views` died with `RangeError: Value is too large to be represented
   as a JavaScript number: -9223372032559808512`, because `db_views.ord` is an order key
   spread from `-2^63` (`command.rs`'s gap-splitting) and `node:sqlite` refuses to hand a
   full i64 to a JS number. The rig printed the stack into the middle of leg I and
   carried on, which is how a *reader* limit looks like a *writer* limit. Fixed in `q.js`
   — plain read first, `setReadBigInts(true)` + stringify on `RangeError` — and re-run.
2. **The first `pipe_lines=0` proved nothing on its own.** With no baseline, "the export
   has no table" and "the page has no table on screen either" are the same observation.
   `mdbase.sh` exists because the control had to show the file is otherwise identical:
   1551 → 1552 bytes, one line, and that line empty.
3. **A guessed schema costs a leg.** `databases` has no `ord` column and `db_views`'
   layout column is `layout`, not `kind`; two queries in the first draft errored before
   the PID gate ran. Fixed by reading `pragma_table_info` for all six tables first — the
   same order the record cites them in now.
