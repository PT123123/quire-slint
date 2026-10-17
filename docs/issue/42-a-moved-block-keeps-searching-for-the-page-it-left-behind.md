# 42 · a block moved to another page keeps searching for the page it left — and when that page is deleted the block stops being findable for good

**Severity: high** — the full-text index stores which page each block is on
(`search_blocks.page_id`, `migrations.rs:128-131`), and the one change that
re-keys a block, `Change::BlockMovedToPage`, never touches that column.
`repository.rs:867` holds the only `UPDATE blocks SET page = ?` statement in the
repository, and the arm that runs it (`:864-882`) writes `blocks` and
`block_children` and returns; `search_index::upsert_block` (`search_index.rs:134-145`),
the only writer of `page_id`, is called from exactly two places — `insert_block`
(`repository.rs:633`) and `BlockTextSet` (`:822`). So the file contradicts its own
header comment, which claims the two tables are "maintained inside the *same
transaction* as the rows they mirror, so the index can never drift from the
document (ADR-0014)" (`search_index.rs:1-4`).

Measured, end to end, against this harness' own session: a one-block move leaves
one row pointing at the page the block left; a whole page moved leaves **5 rows of
5** pointing there; searching the page that now holds those blocks returns `[]`
while the page they left returns all four hits, still titled `sdrsub-P1-SUBSRC`.
Then the page they left is deleted, and `prune` (`search_index.rs:160-173`,
`DELETE FROM search_blocks WHERE page_id NOT IN (SELECT id FROM pages) …`) deletes
the mirror rows of five blocks that are alive and on screen — after which the text
of that page is unfound by the app's own query **permanently**: a restart does not
heal it, an ordinary sync round does not heal it, and an edit heals exactly the one
block that was edited (4 of 5 rows still missing).

## Evidence

`bash sdrmove.sh` → `ftest/sdrmove.log`, `bash sdrsub.sh` → `ftest/sdrsub.log`,
`bash sdcov.sh` → `ftest/sdcov.log`. Helpers: `mksnap.js` (edits the snapshot the
app itself emitted), `announce.js` (the discovery packet that gets the mock peer
admitted), `getsave.js` (binary-safe GET — see the harness section), `postsnap.js`,
`blockpage.js`, `buildmatch.js` (the app's own MATCH-expression builder).

Every arm ran the sandbox debug exe with `--db <case>/quire.db`, gates 5878 on the
listener pid belonging to its own session and 5879 on the same pid before a
datagram is sent, and read every conclusion from a copy of the library
(`quire.db` + `-wal` + `-shm`) after a graceful `--quit`. `%APPDATA%\Quire\quire.db`
was never opened; nothing under `src/`, `ui/`, `install/`, `justfile` or
`Cargo.toml` was modified.

The two doors that produce this change are the ⋮⋮ "Move to" menu
(`controller.rs:5172` → `state.rs:2719` → `command.rs:2365`) and the three-way
merge's `sync_block_diff` (`state.rs:16584`). This rig drives the second one,
because it is the door a headless harness can knock on; the first is cited from
those three call sites and never pressed.

### A. The invariant, in the file's own words, and the one writer it skips

```
search_index.rs:1-4   "…maintained inside the *same transaction* as the rows they
                       mirror, so the index can never drift from the document (ADR-0014)."
grep 'UPDATE blocks SET page =' repository.rs   ->  one hit, line 867
grep 'search_index::' src/  (all writers)       ->  :428 prune  :584 insert_page  :633 insert_block
                                                    :672 PageTitleSet  :822 BlockTextSet  :143 matches (read)
```

`upsert_block` re-derives the page from the row it just wrote:

```sql
INSERT OR REPLACE INTO search_blocks (rowid, page_id, text)
VALUES (?1, (SELECT page FROM blocks WHERE id = ?1), ?2)     -- search_index.rs:139-140
```

That sub-select is the reason the bug is invisible on a single device: the next
keystroke in the moved block re-writes the row with the correct page. The move
itself says nothing to the index; the *edit after it* does, by accident.

### B. One block (`sdrmove.sh`)

Seed through the app's own `--open` door: pages 119 (`sdrift-P1-DRIFT-SOURCE`) and
120 (`sdrift-P2-DRIFT-DEST`), `with_text 129 / indexed 129`, `disagree 0`, and the
marker findable as `page_id 119 … block 1000000146 … score -5.19`.

A peer then hands back the same library with that one block re-filed — after a
**verbatim round first**, because with no shadow the merge treats the re-filed row
as a brand-new block id (see the harness section):

```
round 1 verbatim      -> 200 | block 1000000146 page 119
round 2 re-filed      -> 200 | block 1000000146 page 120
store                 : [{"id":1000000146,"page":120},{"id":1000000147,"page":119}]
DISAGREEING rows      : [{"id":1000000146,"blocks_page":120,"index_page":119}]
mirror rows gone      : []
after a restart       : [{"id":1000000146,"blocks_page":120,"index_page":119}]   ← opening does not heal
the app's own query:
  global              : [{"page_id":119,"title":"sdrift-P1-DRIFT-SOURCE","block":1000000146}]
  in-page 120         : []                    ← the page that holds it
  in-page 119         : [{"page_id":119,"block":1000000146}]   ← the page that lost it
one incoming text edit: DISAGREEING [] and the hit now reads page_id 120 / "sdrift-P2-DRIFT-DEST"
```

### C. The whole page, five blocks (`sdrsub.sh`)

Page 119 seeded with a `heading_1` and four paragraphs (`1000000145…149`), all five
blocks carried to page 120 by one round. `mksnap.js moveall` reports what it changed
and refuses if the count is not the seed's own block count:

```
  5 blocks re-filed from page 119 to 120: 1000000145,…146,…147,…148,…149
  DISAGREEING rows (5 of 5): every one of them, blocks_page 120 / index_page 119
  zebrasub1001 global  : [{"page_id":119,"title":"sdrsub-P1-SUBSRC", …}]
  zebrasub1001 in-page 120: []
  zebrasub1002 global  : [{"page_id":119,…}]     in-page 120: []
  zebrasub1003 global  : [{"page_id":119,…}]     in-page 120: []
  zebrasub1004 global  : [{"page_id":119,…}]     in-page 120: []
```

`core/command.rs:2359-2372` pushes one `BlockMovedToPage` **per block of the moved
subtree**, so this is the ordinary shape of a move, not a worst case: a five-block
page drifts five rows, and a page with 200 blocks drifts 200.

### D. Deleting the page they left turns drift into loss (`sdrsub.sh` ARM2/ARM3)

A third round drops page 119 from the peer's page list; the merge deletes the page
(`state.rs:16014-16019`), the batch contains `PageDeleted`, so `prune` runs
(`repository.rs:427-429`):

```
pages named SUB : [{"id":120,"title":"sdrsub-P2-SUBDEST"}]      ← 119 is gone
blocks surviving: 4 (+ the heading)                              ← the document still holds all five
DISAGREEING now : 0                                              ← nothing "disagrees", the rows are gone
MIRROR ROWS MISSING for blocks that hold text:
  1000000145 page 120 page_exists 1  "P1-SUBSRC"
  1000000146 page 120 page_exists 1  "zebrasub1001 is the "
  1000000147 page 120 page_exists 1  "zebrasub1002 is the "
  1000000148 page 120 page_exists 1  "zebrasub1003 is the "
  1000000149 page 120 page_exists 1  "zebrasub1004 is the "
zebrasub1001/2/3/4 global : [] [] [] []
```

Two things follow, both measured. **The sweep cannot tell a stale key from a dead
source** — `prune`'s `page_id NOT IN (SELECT id FROM pages)` was written for rows
whose block is gone, and a drifted row aiming at a deleted page lands in the same
set. And **no door repairs it**:

```
an ordinary round afterwards : [{"id":1000000146,"page":120,"page_exists":1}]   ← still missing (`sdrmove` ARM7)
one incoming text edit       : still missing 4 of 5; the edited block alone comes back
grep 'search_index::rebuild' ->  one caller: migrations.rs:134, v2's `backfill`
grep 'search_index'  src/    ->  the desktop crate imports `SearchRequest` only (state.rs:40, bin/quire_typing.rs:22)
```

A library at v30 (`migrations.rs:53`) never re-runs v2, so the one function that
would rebuild the mirror is unreachable at runtime for exactly the libraries that
need it.

## Cause

`Change` carries the new key but not the text, and `apply_one` maintains the mirror
per change type rather than per affected row. `BlockTextSet` has the text in hand, so
it can call `upsert_block`; `BlockMovedToPage` has only `id, page, parent, order`, so
nobody thought it needed a mirror write — and the mirror's `page_id` column is the
part that does need one. The result is a column that is correct at insert time,
refreshed by any later text write, and stale in between.

Two design facts make it stick rather than annoy:

- the read path **trusts the index for the page** — `matches` (`search_index.rs:266`)
  joins `pages AS p ON p.id = h.page_id` (`:289`) and reports `p.title` as the hit's page,
  so a stale key is shown to the user as the page's name;
- `prune` treats a stale key as an orphan, so the drift is not merely wrong but
  load-bearing for a delete.

## What this is not

- **Not a coverage complaint.** The mirror has two sources, and the app says so in
  the UI (`ui/components/SearchPanel.slint:102` "搜索页面…" vs `ui/components/DatabaseView.slint:241` "搜索此视图",
  `ui/components/OrganizerArea.slint:1753/3001` "搜索笔记"/"搜索任务"). `sdcov.sh` measured, with the
  app's own query shape (`buildmatch.js` transcribing `search_index.rs:26-96`), that a
  term living only in `db_values.text` ("Sort in SQL"), in a note body (`假标题`), in a
  note title, in a task title, or in `marks.url` returns `[]` — rows that exist, zero
  index rows containing them. That is the documented boundary, and
  `storage/database_query.rs:454-456` states it outright: the mirror "index[es] page titles and block texts, and nothing at all indexes `db_values`" — and it names the reason in the same breath, "a write path per keystroke and a prune rule per bulk path", which is exactly the maintenance this record finds missing.
  One leg is worth recording anyway: `quarterly-report.pdf` **is** findable, not because
  `attachments.name` is indexed but because the file block duplicates the name into
  `blocks.text` (`{"src":"attachment",…},{"src":"block","id":1000000146,"what":"quarterly-report.pdf"}`).
- **Not Ctrl+F.** The find bar builds its session from the blocks the editor already
  holds — `FindSession::new(term, &blocks)` over `doc.borrow().page_blocks(page)`
  (`state.rs:2247-2250`, `find_service.rs:41-57`, "it does no IO") — so in-page find on
  the open page is immune to the drift. What `[]` means above is the **search panel's**
  page-scoped query (`SearchService::query_in_page`).
- **Not the blob door.** The app has a second, repo-less search
  (`Workspace::search`, `workspace.rs:839`, ASCII-fold `find_ci`) that reads the
  in-memory document and is therefore drift-immune; its hits carry `hit_block_id: -1`.
- **Not `BlockMoved`.** Same-page reorder leaves `blocks.page` alone, so the mirror stays
  right. `BlockMovedToPage` is the only re-keying change, and `grep` says so.
- **Not a merge defect.** `sync_block_diff` wrote the right page into `blocks`; the index
  simply was not told. `41`'s and `21`'s sync complaints are about the wire, not this.
- **Not templates.** The template filter is a read-path predicate (`p.template = 0`),
  so template rows sit in the mirror harmlessly — unrelated to a stale `page_id`.

## Candidate fixes

Order matters: 1 alone is not enough, because 2 is what turns the wrong answer into a
lost one.

1. **Tell the index in the arm that re-keys.** Add a `search_index::retarget_block(tx, id,
   page)` doing `UPDATE search_blocks SET page_id = ?2 WHERE rowid = ?1` and call it from
   the `BlockMovedToPage` arm. `page_id` is `UNINDEXED`, so no tokenising work is involved;
   whether that UPDATE is cheap on an FTS5 table needs a probe, and the safe fallback is the
   existing `upsert_block` with the text read back in the same transaction
   (`SELECT text FROM blocks WHERE id = ?1`) — same shape as the sub-select `upsert_block`
   already uses for the page. Undo needs nothing extra: `command.rs:2371` emits the reverse
   `BlockMovedToPage`, and the fix runs on both.
2. **Make `prune` re-key instead of delete, or narrow it.** The sweep's second clause
   (`rowid NOT IN (SELECT id FROM blocks)`) is the one that means "the source is gone";
   the first clause is a *consistency* claim about a column nothing maintains. Dropping the
   first clause leaves drift wrong but recoverable; making it an `UPDATE … page_id =
   (SELECT page FROM blocks …)` repairs the whole library on any delete batch.
3. **A runtime repair door, and an open-time check.** `search_index::rebuild` already does
   exactly the right thing and is unreachable; expose it (a `--reindex` flag, a command) and
   call it on load when the invariant query is non-empty —
   `select count(*) from blocks b join search_blocks s on s.rowid = b.id where b.page <> s.page_id`
   — which is also the integrity assertion this repository has never had. On the drifted
   library above it returns 5 with the fix absent and 0 with it present, so it is a gate, not
   a diagnostic.
4. **Stop letting the read path name the page from the stale column.** `matches` already
   joins `blocks AS b` (`search_index.rs:290`); taking the hit's page from `b.page` instead of
   `h.page_id` makes the panel's title and its scroll target correct for any index that is
   behind, and turns `search-open-hit` (`controller.rs:3014-3038`, which opens the page the hit
   names and then selects a block that page may not hold) into a no-op rather than a wrong jump.
5. **The test that would have caught it.** Nothing in `tests/` applies `BlockMovedToPage`
   and then queries the index; a three-line core test — move, then assert
   `search_blocks.page_id = blocks.page` for the moved id — fails today and passes with 1.

## Not measured

- **The ⋮⋮ menu itself.** `controller.rs:5172` → `state.rs:2719` → `command.rs:2365` is read,
  not pressed; driving it needs desktop UI automation, which this pass does not do. The move
  was produced through the merge door, which pushes the same `Change`.
- **A nested subtree.** `command.rs:2359-2365` emits one change per block of the subtree; the arms
  here moved five *flat* markdown paragraphs. The count claim is a code reading plus a
  five-block measurement, not a deep-tree measurement.
- **Two real devices.** Every round here is one session on `127.0.0.1` receiving a snapshot
  this harness edited (`21`/`37`'s same caveat, restated because this record's move arrives
  over that socket).
- **The user's library.** It was never opened — every arm used its own throwaway `--db` —
  so how many rows sit drifted on this machine today is unknown, and arm C's own lesson is
  that a drifted row is invisible until the page it names is deleted.
- **Pixels.** No search-panel screenshot of a drifted hit, no observation of what
  `search-open-hit` visibly does when the block is not on the page it opens.
- **`task_lists.name`.** The table exists but no library this pass built has a row in it
  (`sdcov.sh` prints `list_rows 0`), so that source is unmeasured; what can be said of it is
  the call-site fact in A.
- **The organizer's own doors** ("搜索笔记", "搜索任务") and the view's "搜索此视图": each is a
  different query over its own table and is what makes the coverage boundary in
  "What this is not" a boundary rather than a hole. None was driven.

## How the harness got it wrong first

- **Every push answered `409`** — 「DESKTOP-HCPV6H6 推送了快照，但它从未在网络上广播过 —
  已拒绝」. An inbound push is refused unless the sender id is already in
  `settings.sync.peers` (`controller.rs:742-749`), which UDP discovery fills
  (`transport.rs:110-131`). `announce.js` is the packet that gets the mock peer admitted, and
  the first version of it died on `Error: setBroadcast EBADF` because `setBroadcast` was called
  before `bind` — a silent failure, since the rig still reported the 409 and looked like the
  merge was the problem.
- **The merge duplicated the block instead of moving it.** `merge` is three-way and its base
  is the per-peer shadow — `sync.shadow.<peer id>` read at `state.rs:15803` and handed to
  `crate::services::sync::merge::merge(&local, shadow, remote, …)` at `:15887`. With no shadow
  for a brand-new mock peer there is no base, so the re-filed row arrived as a *new* block id
  (1000000150) while the original stayed on page 119 and `sync_block_diff`'s move branch never
  ran — `DISAGREEING` stayed empty, which reads as "no defect". A verbatim round first is what
  a real first sync of two devices is, and it is what makes the move land on the original id.
- **A missing fixture made an arm meaningless**: only `sdrift-P1-DRIFT-SOURCE.md` existed, so
  `cp` failed, the destination page came back empty, and `mksnap.js` produced
  `"page":undefined`. `mksnap.js` now ends with a `JSON.parse(out)` self-check and every call
  site has an `|| exit 5` guard.
- **Edits are textual, never a JSON round trip.** `ord` is a full i64 order key; parsing the
  body in JS would round it and make the merge report a hundred unrelated moves, burying the
  one being measured. `moveall` is bounded to the `"blocks"` array by a string-aware depth
  walk because `SRecord` serialises a `page` field too (`services/sync/model.rs:212-215`) — a
  body-wide `"page":119` replace would move database rows as well.
- **`getfile.js` would have corrupted the titles it was checking** (`buf += d` utf-8-decodes a
  chunk boundary); `getsave.js` keeps bytes, which matters here because a mangled page title
  makes the merge rename a page and the arm measures something else entirely.
- **A raw hyphen in a MATCH needle is SQLite, not Quire.** Pasting `P2-SUBDEST*` into the
  query answered `no such column: SUBDEST` — the app never sees that, because `build_match`
  wraps a non-bareword term in quotes. `buildmatch.js` exists so the needle reaching `MATCH` is
  the app's own expression, and it is the reason the CJK leg reads `"假 标 题"*` rather than a
  `假标题` that would have matched nothing and looked like a second defect.
- **`buildmatch.js`'s first version lacked the `u` flag** on its bareword regex, so `\p{L}`
  degenerated to a literal and *every* Latin query became an exact-phrase query with no
  trailing `*`. The control leg caught it: a term known to be indexed came back `[]`.
