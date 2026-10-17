# 33 · the organizer's MCP doors speak two tag grammars, and one id numbers two tables

**Severity: medium** — `create_note` and `create_task` take the same `tags` string and
store different sets of tags, because one of them uses the app's tag parser and the
other has its own splitter. `Machine Learning` is two tags on a note and one tag on a
task. Worse, the task door can *create* a tag the task door cannot *keep*: the update
path runs the space-splitting parser, so the row's own tags change when any other
field of it is edited. And `notes.id` and `tasks.id` are two independent sequences
starting at 1, both returned as a bare `{"id":N}`, so `update_task {task_id:3}`
renames task 3 while the agent means the note it just listed with id 3 — no error, no
warning, one wrong row written.

## Evidence

Two batteries against one throwaway `--db` each, one bounded session, every call
sequential and 400 ms apart so the 250 ms `tools/call` pump (`24`) cannot be mistaken
for tool behaviour, and everything read back from a snapshot copy of the file after a
graceful `--quit` (`orgprobe.sh` + `orgprobe.js`, then `org2.sh` + `org2.js`).

**A. One string, two doors, two tag sets.** `org2.js` legs A/B/E/F:

```
create_note {title:"G1-note-space", tags:"Machine Learning,项目"}  -> {"id":1}
create_task {title:"G2-task-space", tags:"Machine Learning,项目"}  -> {"id":1}
create_note {title:"G4-note-spaceonly", tags:"Machine Learning"}   -> {"id":2}
create_task {title:"G5-task-spaceonly", tags:"Machine Learning"}   -> {"id":3}
```

the file afterwards, `select id,quote(tags) from notes/tasks`:

```
notes  1 'G1-note-space'      tags '["Machine","Learning","项目"]'      ← 3 tags
notes  2 'G4-note-spaceonly'  tags '["Machine","Learning"]'             ← 2 tags
tasks  1 'G2-task-space'      tags '["Machine Learning","项目"]'        ← 2 tags
tasks  3 'should-fail'        tags '["Machine Learning"]'               ← 1 tag
```

The shortest pair is the clearest: the single word-phrase `Machine Learning` becomes
two tags on a note and one tag on a task. Same tool family, same documented field
(`mcp_tools.rs:274`: 「tags 用逗号分隔多个标签」), different store.

**B. A task tag with a space cannot survive its own update door.** Same battery,
legs C/D on one row:

```
create_task {title:"G3-task-edit", tags:"Machine Learning,项目"} -> {"id":2}
update_task {task_id:2, tags:"Machine Learning,项目"}            -> {"ok":true}
```

```
tasks 2 'G3-task-edit' tags '["Machine","Learning","项目"]'
```

`create_task` had just stored `["Machine Learning","项目"]` for that exact string (leg
B proves it on a twin row), and `update_task` with the *same* string stores three
tags. So the two doors disagree on one row, and the row's tags depend on which verb
last touched it. The display side makes this a user-visible mutation rather than an
agent curiosity: `tags_text` joins a tag list with a **space**
(`state.rs:17055-17057`), which is what the tag input is seeded with
(`org_task_tags_text`, `state.rs:8598-8604`), and `parse_tags` splits on a space
(`state.rs:17043`) — so a task holding `["Machine Learning","项目"]` is shown as
`Machine Learning 项目`, and the next edit of that field writes three tags. The join
and the parse are read off the two functions; the split itself is measured above, in
the one door this pass can drive.

**C. Why they differ.** Two splitters, one of them hand-rolled inside the tool:

```rust
// mcp_tools.rs:456-464, create_task
tg.split(|c| c == ',' || c == '，').map(str::trim).filter(|s| !s.is_empty())

// state.rs:17043, parse_tags — what org_note_tags and org_task_tags both call
input.split([' ', '\n', '\t', ',', '，', '、'])
```

`create_task` splits on two separators and then `trim()`s; `parse_tags` splits on six
(separators, and the newline/tab cases mean `tags:"a\nb"` is one tag at create and two
at update), and also de-duplicates (`:17045`), which `create_task` does not — so
`create_task {tags:"工作,工作"}` can store the same tag twice. Not measured: that leg
was not run.

**D. `notes.id` and `tasks.id` are different tables with the same numbers, and no
tool says which one it wrote.** Both sequences start at 1 in a fresh library:

```
create_note {title:"G1-note-space", ...}  -> {"id":1}
create_task {title:"G2-task-space", ...}  -> {"id":1}
```

and `note_row`/`task_row` both serialise the field as `"id"`
(`mcp_tools.rs:361`, `:430`). So an agent that calls `list_notes`, picks id 3, and
passes it to a task tool hits a real, different row:

```
update_task {task_id:3, title:"should-fail"}  -> {"ok":true}
trash_task  {task_id:3}                       -> {"ok":true}

tasks 3 'should-fail' tags '["Machine Learning"]' deleted_at 1792205577
notes 3 'G6-note-only-id' body 'x'                deleted_at null      ← untouched
```

The task with that number was renamed and sent to the 回收站; the note the caller was
thinking of is unchanged, and neither answer mentions the row count or the title it
acted on — `{"ok":true}` is the whole response. The guard works in the other
direction only because the tables have different *lengths*:

```
update_note {note_id:4, title:"should-fail"}  -> tool-error "笔记 4 不存在。"
trash_note  {note_id:4}                       -> tool-error "笔记 4 不存在（或已在回收站）。"
restore_note{note_id:4}                       -> tool-error "笔记 4 不在回收站。"
```

`orgprobe.sh`'s first pass designed this leg wrong and it is worth recording why: it
handed a *task* id to a note tool, and both tables happened to have that id, so the
call legitimately wrote a note and proved nothing about collisions. `org2.js` sizes
the tables apart first. The three error texts above are good work — `update_*` checks
existence (`:395`, `:487`) and `restore_*` checks trashedness — the hole is only that
the two tables share a number space with no type in the answer.

**E. The rest of the battery came back clean, and is recorded so the next pass does
not re-spend on it.** From `orgprobe.log`:

- `include_trashed` behaves: with nothing trashed, `list_notes {}` and
  `list_notes {include_trashed:true}` both return 2; after one trash they return 1
  and 2, and the second carries `"trashed":true`. Same shape on tasks.
- `update_note {tags:""}` clears the tags to `[]` and answers `{"ok":true}`;
  `update_note {body:...}` writes the body. No surprise.
- `due:""` through `update_task` stores **NULL**, not `""` (`state.rs:8766-8767`), and
  `list_tasks` returns `"due":null` — so the empty-string hole I went looking for
  does not exist.
- `done:true` then `done:false` leaves `completed_at null`
  (`state.rs:8733`, `done.then_some(now)`), measured on task 6.
- `priority:"HIGH"` is accepted and stored as `high` (`priority_slot`, `:306-312`) —
  matching `23`'s note that the tool surface lowercases where the HTTP auth does not.
- Full-width commas split (`工作，私人，家庭` → 3 tags), and so does `、` in
  `parse_tags` but **not** in `create_task` (`:458` only knows `,` and `，`) — so
  `create_task {tags:"a、b"}` stores one tag `a、b` where the note door stores two.
  The note door's answer for that string was not run.
- Bodies keep newlines, tabs and Markdown-looking text verbatim (`第一行\n第二行\n\n###
  假标题\n- 列表项`, `a\tb`) and `list_notes` returns them as written, which is the
  documented behaviour since SPEC's 笔记 body decision (正文不渲染 Markdown).
- `create_task {list_id:-5}` stores `list 0` (the inbox), read from
  `state.rs:8690`'s `if list >= 0` — while `17` measured `list_id:999999` stored
  verbatim as `999999`. Two answers to "that list is not here", one clamp and one
  silent orphan.

## F. Fields the surface can read but not write

`tool_defs()` (`mcp_tools.rs:108-215`) is the whole boundary, and the read shapes are
wider than the write shapes:

- `note_row` returns `pinned` (`:365`); no `create_note`/`update_note` parameter
  touches it. A note can be listed as pinned and never set or cleared by an agent.
- `task_row` returns `list`, `subtasks` and `repeat` (`:431-438`); `create_task` takes
  `list_id` but **`update_task` has no `list_id`** (`:183-196` lists task_id, title,
  notes, due, priority, done, tags), so an agent can put a task in a list and never
  move it — although the app has the command (`org_task_move`'s neighbour, the
  comment at `state.rs:8786-8788` describes moving a task to another list, `-1` =
  inbox). `subtasks` and `repeat` are likewise read-only from the door.
- The 提示词 the MCP page hands out (`agent_prompt`, `:221-281`) is generated from
  `tool_defs()`, so it can never name a parameter that does not exist — it is
  consistent with the surface by construction. What it does promise and this record
  contradicts is one line: 「tags 用逗号分隔多个标签，支持「项目/子标签」这样的层级写法」.
  Measured above: for notes the separator list is six characters including the space,
  so a note tag cannot contain a space at all, and the comma-only sentence is wrong
  for the door an agent is most likely to use first.

## Cause

- `mcp_tools.rs:456-464` — `create_task` parses tags itself instead of calling
  `parse_tags`, so the two doors have two grammars. `create_note` does *not* hand-roll
  it: `:389` → `org_note_tags` → `parse_tags` (`state.rs:8556`).
- `state.rs:17055` joins with a space while `:17043` splits on a space: the
  serialise/parse pair is not an inverse, so a tag list cannot round-trip through its
  own text form once it contains a space.
- `mcp_tools.rs:361` and `:430` both emit `"id"`, and `org_create_note` /
  `org_create_task_tagged` draw from two independent counters, so nothing in the
  response distinguishes the tables, and every `arg_i64` accept is unconditional.
- `agent_prompt` is built from `tool_defs()` descriptions (`:227`), so the tags
  sentence in the prompt is a hand-written string in a file whose *parameters* are
  generated — the one thing that drifts is exactly the thing that is prose.

## Not measured

- **The UI's tag input.** This pass drives no desktop UI, so leg B's "the next edit
  writes three tags" is the mechanism read off `tags_text` + `parse_tags` plus the
  same split measured through `update_task`. No pixel claim.
- **`create_task {tags:"工作,工作"}`** (the dedupe asymmetry) and
  `create_task {tags:"a、b"}` — predicted from `:458` vs `:17043`, not run.
- **Does the 回收站 hold a note and a task with the same number at once?** Both
  `deleted_at` columns are per-table, so yes by construction; not driven.
- **The `已导入`/notice-band text** for these calls: the MCP page's log line comes from
  `log_message` (`:530`), which was not photographed.

## Candidate fixes

- **One splitter.** Delete the hand-rolled split at `mcp_tools.rs:456-464` and pass
  the raw string through the same `parse_tags` the other doors use — one line, and
  `create_task`/`update_task` stop disagreeing. If a task is *meant* to keep spaces in
  a tag, then `parse_tags` is the one that should change, and `tags_text` must join
  with something it cannot split on (`, `), or the text form has to escape — otherwise
  the field's display form is lossy for the values the create door can write.
- **Say which row you wrote.** Make the write answers self-describing:
  `{"note_id":3,"title":"G6-note-only-id"}` instead of `{"id":3}` / `{"ok":true}`.
  `log_message` already assembles a human sentence out of the args (`:530-533`), so
  the ingredients are in the file. It costs an agent nothing to read and makes the
  wrong-table call visible in the transcript instead of two rows later.
- **Reject, or accept and say so, when a `task_id` names a note.** A real guard is not
  possible across two int sequences, but the tool can check the other table and answer
  `任务 3 不存在（笔记 3 存在：要改笔记请用 update_note）` — one lookup, no new state.
- **Give `update_task` the `list_id` it already documents having** (the description at
  `:184` says 「修改一个任务的标题/备注/截止日/优先级/完成状态/标签」 — honest, but the
  UI can move a task and the door cannot), and either add `pinned` to the note tools
  or drop it from `note_row`.
- **Fix the prompt's tags sentence**, or better, generate it: the separator list is a
  fact about `parse_tags`, and a hand-written sentence about it in the same crate is
  how it went stale.
- **`list_id` should answer one way.** `17`'s fix (reject an unknown list) plus this
  record's clamp (`-5` → inbox) can be one rule: a `list_id` that is not `0` and not a
  row is an error, negative or not.
