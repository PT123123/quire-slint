# 18 · the `due` field answers `{"ok":true}` to a date it refused to store

**Severity: medium** — three different outcomes for one argument, two of them
silent. The user-visible half is a deadline that quietly never moved; the
store-visible half is a date that has never existed in any calendar, which the
row's own 逾期 badge then compares as a string.

## Evidence

One row, created with a real deadline, then each candidate value pushed through
`update_task` and read back from the same session
(`.scratch/issue-test/ftest/duecheck.js`, library `ftest/due1/`):

```
created task 1 with due 2026-12-31
update_task due="明天"              → tool-result {"ok":true}   row.due="2026-12-31"
update_task due="2026-12-31T08:00"  → tool-result {"ok":true}   row.due="2026-12-31"
update_task due="2026-13-45"        → tool-result {"ok":true}   row.due="2026-13-45"
update_task due="2026-1-4"          → tool-result {"ok":true}   row.due="2026-13-45"
update_task due="0000-00-00"        → tool-result {"ok":true}   row.due="0000-00-00"
update_task due=""                  → tool-result {"ok":true}   row.due=null
create_task {due:"明天"}            → tool-result {"id":2}      row.due=null
```

Read that as three behaviours, none of them reported:

| what was sent | what happened | what the caller was told |
|---|---|---|
| `明天`, `2026-1-4`, `2026-12-31T08:00` | nothing — the old date stayed | success |
| `2026-13-45`, `0000-00-00` | stored verbatim | success |
| `""` | cleared | success |

The earlier battery (`mcp-e2e.js` checks `K3`/`K4`/`K5`) saw the first group as
"the due became null"; it had created every row with no date, so "untouched" and
"cleared" looked the same. This run is the version that separates them.

## Cause

`src/app/state.rs:8764-8774` is honest about the shape — it is the *caller* that
drops the answer:

```rust
pub fn org_task_due(&self, id: i64, due: String) -> Option<Vec<Change>> {
    let due = due.trim().to_string();
    let due = if due.is_empty() { None }
        else if crate::core::is_iso_date(&due) { Some(due) }
        else { return None };                     // ← the refusal is an Option
    self.org_edit_task(id, |t| t.due = due.clone())
}
```

`mcp_tools.rs:477` (`create_task`) and `:497` (`update_task`) both call it as a
statement, so `None` is discarded and the function goes on to
`Ok(json!({ "id": id }))` / `Ok(json!({ "ok": true }))`. Every other organizer
helper in that file has the same ignored-`Option` shape (`org_task_notes`,
`org_task_tags`, `org_note_title`), which is why `create_note`'s `let _ =` at
`:384-390` hides the same class of refusal.

The two impossible dates come from the validator itself. `quire-core` at the
pinned rev (`Cargo.lock:3907`, `db0e95f`), `src/core/date.rs:23-31`:

```rust
pub fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}
```

Shape only: month 13, day 45 and `0000-00-00` all pass, and the doc comment two
lines above it says "a multi-byte character can never be an ASCII digit, so
`len() == 10` plus the two dashes is the entire rule" — which is true of the
*bytes* and not of the *calendar*.

Downstream, the date is compared as text. `state.rs:16949-16951`:

```rust
fn org_overdue(due: Option<&str>, dates: &OrgDates) -> bool {
    matches!(due, Some(d) if d < dates.today.as_str())
}
```

so `2026-13-45` is forever in the future ("13" > "10" as a string) and
`0000-00-00` is forever overdue. `org_due_label` (`state.rs:16931-16946`) paints
the stored string for anything that is not today/tomorrow/yesterday, so the row
shows `2026-13-45` in the list exactly as it was written.

## Candidate fixes

- Make the refusal visible: have the two `org_task_due` call sites read the
  `Option` and return `Err(format!("无法识别的日期：{d}（要用 YYYY-MM-DD）"))`. One
  `if … .is_none()` per site. Nothing else about the tool changes; a caller that
  sent `明天` gets a message instead of a lie. (This is the same argument the
  record about `17` makes from the other side: the reply must be produced after
  the writes it claims succeeded have happened.)
- Make `is_iso_date` mean what its name says: month `01..=12`, day
  `01..<days_in_month(year]`. It lives in `quire-core`, so it is a two-repository
  change and the shell's 431 core tests are the ones that will notice — the
  `2026-13-45` row above is the fixture to add there. If core cannot move this
  quarter, the shell can reject on its own before calling `org_task_due`, which
  leaves the *picker* path unaffected and closes only the agent door.
- Normalise or reject the date+time spelling
  (`2026-12-31T08:00:00`, what a `datetime-local` input and most agent clocks
  hand over) rather than silently dropping it: `YYYY-MM-DD` prefix + length > 10
  → truncate and say so in the result, or refuse. Today it is refused, and the
  refusal is invisible, which is the worst of the three.
- Add to the battery: after each `due` write, the row's `due` must equal what
  the call returned as success — the assertion that would have caught all three
  behaviours at once. `K4`/`K5` as originally written assert on `row.due`
  against a row that never had a date, which is why they described the symptom
  wrongly.
