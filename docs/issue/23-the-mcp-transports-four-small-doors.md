# 23 · the MCP transport's four small doors: `bearer`, the 404 that answers first, the 202 that did nothing, and the newline that stayed

**Severity: medium** — none of these loses data on its own. Together they are the
difference between "a conforming client works" and "only my client works", and
one of them is a reply that lies about doing the work.

## 23.1 `Authorization: bearer <token>` is refused

```
T6  lowercase "bearer" scheme (RFC 7235: case-insensitive)
    expected 200   observed 401
```

`X-Quire-Token` (T7) and `Bearer` (T4/T5) both behave, so this is not a token
problem. Cause: `src/mcp.rs:225-229`

```rust
let authorized = headers
    .get("authorization")
    .map(|v| v.trim_start_matches("Bearer ").trim() == self.token)
```

`trim_start_matches("Bearer ")` is case-sensitive, so `bearer x` keeps its
prefix and compares the whole string against the token. The same call is
greedy rather than single: `Bearer Bearer <token>` strips both and passes, and a
token that *begins* with `"Bearer "` could not be presented at all — both are
artifacts of using a string trim where a scheme parse belongs.

Fix: split once on whitespace, compare the scheme
`eq_ignore_ascii_case("bearer")`, and take the remainder as the credential. The
header *name* is already lower-cased at `:217`, so the pattern is in the file.

## 23.2 the path check runs before the token check

```
T9  POST to a path other than /mcp → 404 (expected: 404 *after* auth)
```

`mcp.rs:231-237` answers `404 not found` and only then asks `if !authorized`.
An unauthenticated probe against `127.0.0.1:5880` therefore learns that a Quire
is listening, and — because `/sync/info` on 5878 answers without a credential at
all (`21`) — learns which device it is. One-line fix: swap the two blocks so
every wrong-path answer requires a right token. The `405` for non-POST
(`:238-242`) should move with them, for the same reason.

## 23.3 `tools/call` with no `id` is accepted and does nothing

```
A5  tools/call sent as a notification (no id)
    202 (202 Accepted) and rows 0 → 0
```

`src/mcp.rs:126-128` routes every id-less message to `Reply::Silent`, which
`:246-248` renders as `202 Accepted` with an empty body. For
`notifications/initialized` that is correct. For `tools/call` it is a client that
thinks it wrote something: the HTTP status says "accepted", the tool never runs,
and the MCP log gets no line because nothing was executed.

A notification has no reply channel by definition, so the server cannot answer
"no" — the honest options are: (a) refuse `tools/call` without an id at the
JSON-RPC layer with a `-32600` on `200` — a spec deviation in the client's
favour, and the one an SDK author would expect; or (b) keep `202` but log it as a
refused call on the MCP page, so the user can see that an agent asked to write
and nothing happened. (a) is preferable: the alternative leaves the client
guessing. Whatever is picked belongs in `tool_defs()` descriptions too, because
an agent reads those before it sends anything.

## 23.4 a newline inside a title is stored verbatim

```
I2  create_note {title:"第一行\n第二行"}
    → {"id":N};  list_notes returns "第一行\n第二行"
```

`title` is `TEXT NOT NULL DEFAULT ''` in the store (`dbprobe.js <db> notes`
prints the schema) and the organizer's row is a fixed-height `Text` with
`overflow: elide` (`ui/components/OrganizerArea.slint:76-90`, `height: 22px`), so
a value carrying `\n` is displayed by a component that was not written for one.
**This pass did not photograph that row**, so the claim stops at "the store keeps
it and the painting component is a one-line `Text`" — whether it clips, breaks
the line into the next row's height, or draws both lines inside 22 px is
unmeasured. `quire-shot` cannot reach it without a scene that seeds a
newline title, and this pass made no changes to `src/` or `ui/`.

The fix is a one-liner at the boundary if the field is single-line by intent:
`title.replace(['\n', '\r'], " ").trim()` inside `create_note`/`update_note`
(`src/app/mcp_tools.rs:382-408`) — the same place that already decides what a
tag string means. If multi-line titles are *not* intended, they should be
refused rather than flattened, because a flattened title is a search result the
user cannot reproduce.

## Adjacent, and worth knowing before adding tools

Two checks that passed, recorded so the next pass does not re-spend on them:
`I1` a 5,000-character title is stored intact; `I4` CJK and emoji round-trip
byte-exact (an earlier harness run reported these as failures and was wrong —
it decoded the response as `latin1`, see `docs/issue/README.md`'s harness note);
`I6`/`I7` an unknown tool name is refused and no hard-delete verb is reachable
(`delete_note` → `未知工具：delete_note`); `K10` `task_id` as a string is refused
(`缺少或非法参数：task_id`).
