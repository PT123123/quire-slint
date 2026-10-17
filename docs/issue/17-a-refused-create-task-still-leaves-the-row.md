# 17 · a refused `create_task` still leaves the row it refused to fill

**Severity: high** — the agent surface's only non-atomic write. A call that
answers `isError: true` has already minted an id, already recorded the row, and
already put it in the user's 清单. Repeating a call that fails is how an agent
recovers from a typo; here it is how a library fills up with empty tasks.

## Evidence

The battery (`.scratch/issue-test/ftest/mcp-e2e.js`, run against a throwaway
`--db` under `.scratch/issue-test/ftest/mcp2/`), two checks:

```
K6  create_task {title:"ftest 坏优先级", priority:"urgent"}
    → tool-error  未知优先级：urgent（none/low/medium/high）
    → tasks 行数 4 → 5
K7  create_task {} (no title)
    → tool-error  缺少或非法参数：title
    → tasks 行数 5 → 6
K8  create_task {title:"ftest 幽灵清单", list_id:999999}
    → tool-result {"id":7}
```

Read back out of the file with `node dbprobe.js mcp2/snapx/quire.db tasks` (an
independent SQLite handle, not the app):

```json
{"id":5,"title":"ftest 坏优先级","due":null,"priority":"none","list":0,"deleted_at":null}
{"id":6,"title":"","due":null,"priority":"none","list":0,"deleted_at":null}
{"id":7,"title":"ftest 幽灵清单","due":null,"priority":"none","list":999999,"deleted_at":null}
```

All three rows are in the store after the session was killed, so they are not a
transient in-memory state: they sync, they count, and row 6 and row 7 are
reachable through no view — an empty title and a list id no list has.

## Cause

`src/app/mcp_tools.rs:454-484`. The row is created first and the arguments are
validated after it exists:

```rust
let list = args.get("list_id").and_then(Value::as_i64).unwrap_or(0);   // :455
…
let id = state
    .org_create_task_tagged(list, tags)                                // :465-467  ← row exists from here
    .ok_or("任务创建失败。")?;
let title = arg_str(args, "title")?;                                   // :468  ← `?` returns the error, row stays
state.org_task_title(id, title.to_string());
…
if let Some(p) = arg_opt_str(args, "priority") {
    state.org_task_priority(id, priority_slot(p)?);                     // :481  ← same
}
```

`list` at `:455` never asks whether that list id is one the catalog has —
`unwrap_or(0)` also means "no `list_id` given" and "`list_id: 0`" are the same
request, and an unknown number is simply carried into the row.

The same shape is in `create_note` (`mcp_tools.rs:382-392`): `org_create_note()`
runs first and every later `org_*` result is discarded with `let _ =`, so a
note whose title write failed is still a note. The difference is that
`create_note` has no fallible argument left after the create, so it does not
produce the orphan.

## Candidate fixes

- Validate before creating: hoist `arg_str(args, "title")?` and
  `priority_slot(p)?` above the `org_create_task_tagged` call. Two lines moved,
  no new API, and every refusal then happens on a row that was never made.
  `create_note` gets the same ordering for symmetry even though it is currently
  unreachable.
- For `list_id`, decide what an unknown list means and say it in the tool
  description. Two honest options: reject (`list_id` must be one of
  `org_catalog().lists`), or normalise to the inbox (`list = -1`/`0`) and report
  the normalised value in the result so the caller can see it moved. A third
  option — create the list — is a bigger permission and should be its own tool.
- Either way the result should echo what was stored (`{"id":N,"list":N,"title":…}`),
  because `{"id":N}` alone cannot tell a caller that its `list_id` was ignored.
- Pin it: add to the battery "a `tool-error` from `create_task` leaves `tasks`
  count unchanged", asserting on the row count rather than on the reply. That
  check is what this record is missing today — `K6`/`K7` only became visible
  because the harness counted rows before and after.
