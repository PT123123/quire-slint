# 19 · the 回收站 is not a boundary for the agent, and `trash_note`'s answer depends on the wall clock

**Severity: medium** — two halves, one of them about reachability: an agent can
rewrite a row the user has thrown away, and the delete verb's reply changes
depending on which second it lands in.

## Evidence

`mcp-e2e.js` (`W10`), against `ftest/mcp2/`:

```
create_note → id 2
trash_note 2                 → {"ok":true}
list_notes {}                → 2 is not there            (hidden, as designed)
update_note 2 {title:"改了一条回收站里的"} → tool-result {"ok":true}
```

and read back out of the file, independent of the app
(`node q.js mcp2/snapx/quire.db "select id,title,deleted_at from notes where deleted_at is not null"`):

```json
[{"id":2,"title":"改了一条回收站里的","deleted_at":1792194386}]
```

The row is still binned and now carries the agent's text. Nothing refused it, and
`restore_note` would put the agent's edit in front of the user.

`W9`, the same row twice:

```
trash_note 2 → {"ok":true}
trash_note 2 → {"ok":true}        (this battery run)
trash_note 2 → 笔记 2 不存在（或已在回收站）。   (the run before, same binary)
```

The two runs differ by the millisecond the second call crossed a second
boundary. A check whose expected value depends on `now_secs()` cannot be a gate,
so `W9`'s "refused" expectation was wrong as an assertion and right as a
question — the answer is what this record is about.

## Cause

`src/app/mcp_tools.rs:394-397` asks the catalog whether the row exists, and the
catalog answers for every row it holds, binned or not:

```rust
if state.org_catalog().note(crate::core::organizer::NoteId(id.max(0) as u64)).is_none() {
    return Err(format!("笔记 {id} 不存在。"));
}
```

That "includes the bin" is not a bug in `note()` — `org_restore_note` has to
find a binned row to bring it back, so the accessor must see it. It is the
*caller* picking the only accessor there is: `list_notes` filters with
`include_trashed` (`mcp_tools.rs:371-379`) and `update_note` does not, so the
agent's read side hides binned rows while its write side reaches them.
`update_task` (`:487`) has the same shape, one step worse: it does not filter
`trashed` either, it scans `org_catalog().tasks` directly.

The double-trash is `state.rs:8360-8372`:

```rust
let before = self.organizer.borrow().note(NoteId(id.max(0) as u64))?.clone();
let mut after = before.clone();
after.deleted_at = trashed.then(now_secs);
if after == before {
    return None;                                  // ← "nothing changed"
}
```

`None` means two different things here — no such row (`?`) and no change needed
(`after == before`) — and `mcp_tools.rs:412-416` renders both as one message:

```rust
None => Err(format!("笔记 {id} 不存在（或已在回收站）。")),
```

Within the same second, the second trash computes the same `deleted_at` and
takes the `after == before` branch; across a second it restamps and takes the
`Some` branch, which is `{"ok":true}`. `trash_task`/`restore_*` share it
(`state.rs:8374-8386`).

## Candidate fixes

- Make the agent's write side agree with its read side: filter on
  `is_trashed()` in `update_note` and `update_task` and answer
  `笔记 {id} 在回收站里，先用 restore_note 取回。` That message tells the caller
  what to do next, which "不存在" does not. If the intent is instead that the
  agent *may* repair a row the user binned, then `list_notes` should show
  binned rows by default and the asymmetry is the thing to remove — pick one.
- Split `org_set_note_trashed`'s two `None`s. The smallest honest version is to
  have the helper return `Result<Option<Vec<Change>>, NotFound>`-shaped info, or
  — no core change — check existence at the MCP layer first
  (`org_catalog().note(..).is_some()`) and keep `None` meaning "already there",
  so the tool can answer "已在回收站" for it. Idempotency is the thing worth
  deciding on purpose: either a repeat trash is `{"ok":true, "already":true}`,
  or it is a refusal, but it must not depend on the clock.
- Whatever is chosen, make the battery's `W9` assert the *invariant* rather than
  one branch: two trashes in a row must give the same `trashed:true` row and the
  same `deleted_at` value. That is measurable across a second boundary and does
  not care which wording the refusal takes.
- The UI's own 回收站 path is not affected by any of this: 清空回收站
  (`state.rs:8411+`) batches one Ctrl+Z, and hard delete is not on the agent's
  surface (`I7`: `delete_note` → `未知工具：delete_note`). Worth keeping in mind
  when `update_note`'s filter is added — the bin is the only thing between an
  agent and a row the user meant to lose.
