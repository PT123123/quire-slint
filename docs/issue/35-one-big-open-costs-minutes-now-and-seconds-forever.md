# 35 · one big `--open` costs two minutes of dead window, and then forty seconds of every later launch

**Severity: high** — `import_from_path` runs on the UI thread before
`slint::event_loop()` (`src/main.rs:1042`), and inserting a block into a page is
`vec.iter().all(...)` plus a full `sort_by_key` of that page's whole block vector
(`core/document.rs:124-133`) — so importing a 100,000-line markdown is quadratic in the
number of lines. Measured: **128 s** of that, with a window that is on screen and
answers to nothing, `Responding=False` on every sample, the MCP port never opened, and
`--quit` *refused*. The control is the same 5,488,890 bytes on a single line: it reaches
the event loop inside one sample interval. The cost is per block, not per byte. Then the
part that outlives the import: that library, launched again with **no `--open` at all**,
takes **38 s** to become usable, and it takes it on every later launch.

## The curve

`cliprobe2.sh`'s `scale` arm measures the gap between the session line and the
`--dump-state` line, which is the same gap as between "process started" and "the event
loop exists" — `dump-state` prints at `main.rs:1192`, after the import. `bigprobe.sh`
polls the same thing once a second and adds the window state.

| file | lines | bytes | session at | loop at | blocked |
|---|---|---|---|---|---|
| `big1line.md` (control) | 1 | 5,488,890 | 4.5 s | ≤ 4.5 s | **~0 s** |
| `mid10.md` | 10,000 | 498,890 | 3.0 s | 5.6 s | 2.6 s |
| `mid50.md` | 50,000 | 2,538,890 | 2.8 s | 30.3 s | 27.5 s |
| `big.md` | 100,000 | 5,488,890 | 9.4 s | 137.2 s | **128 s** |

5× the lines costs 10.6× the time; 2× the lines costs 5× the time. The control kills the
"it's just a big file" reading: 5.5 MB read once and turned into **one** block is free,
and the same 5.5 MB turned into **100,000** blocks is two minutes.

## What the user sees during those 128 seconds

`bigprobe.sh`'s `big` arm, one line per state change, `hwnd|Responding|has-title|ws`:

```
  t=   9.4s  loop=blocked mcp_down hwnd|responding|has-title|ws=4653302|False|False|83MB
  t=  58.2s  loop=blocked mcp_down …|False|False|110MB
  t= 106.5s  loop=blocked mcp_down …|False|False|206MB
  t= 133.7s  loop=blocked mcp_down …|False|False|267MB
  t= 137.2s  loop=blocked mcp_up   …|True |False|222MB
  t= 142.8s  loop=loop_reached mcp_up …|True|False|278MB
```

Four things in that transcript, all of them the app's, none of them the harness's:

- **A window exists and is not responding.** `MainWindowHandle=4653302` from the first
  sample onward, `Responding=False` for every one of the 128 seconds. `Responding` is
  the same flag Windows uses to decide whether to paint the title bar "(无响应)", so the
  honest description of the state is: the editor appears, and is greyed out as frozen.
- **The agent endpoint never comes up while it is blocked.** `listeners=` shows only
  `0.0.0.0:5878`; `127.0.0.1:5880` appears at t=137.2 s, i.e. after the import. The MCP
  server starts at `main.rs:1085-1140`, below the dispatch.
- **`--quit` is refused.** The first `big` arm (`cliprobe-flags.log`) asked the blocked
  session to exit and got: `Error: "the instance refused the request: it could not
  schedule its exit"` with rc=1. The quit handler posts to a loop that does not exist
  yet, which is the same reason `--auto-exit` cannot save it — its timer is armed at
  `main.rs:1212`, below the dispatch too.
- **Memory climbs the whole way**: 83 MB → 278 MB working set for 5.5 MB of text, then
  back to 239 MB once the loop starts.

And when it does finish, the arm's own numbers say the write path is fine: `--quit`
drains in **1.1 s**, `session ended` is in the log, and the file holds
`pages=20 blocks=100145 block_children=100145` with `on_newest=100000` — every block, on
one page, no loss. The defect is the 128 seconds, not the data.

## The 38 seconds that stay

`relaunch.sh` copies that arm's library (`bigprobe/big/quire.db` + `-wal`) and launches
it with `--db … --dump-state` and **no `--open`**:

```
session_at=   2.0s pid=171312
loop_reached_at=  39.9s  last_blocked_state=6953094|True|166MB
console=[quire: mcp server on 127.0.0.1:5880|dump-state: pages=20 …]
log: … session started (pid 171312)|… session ended|
[{"pages":20,"blocks":100145,"newest":"'big'"}]
```

So the import is a one-time 128 s and the load is a permanent 38 s. The load path
reports `Responding=True` where the import path reported False, which is why this arm is
written as "38 s before the loop exists" and not as "another dead window" — measured
difference, unmeasured reason.

One more honest number from the control arm: `big1line.md` — a page whose *one* block is
5.5 MB of text — writes a 13,893,632-byte library and a 14,560,112-byte WAL. So roughly
2.6× the source size is what the store costs even in the easy case; the 100 k-block arm
costs 30 MB + 31 MB for the same text, and the difference is per-row overhead, not copy.

## Cause

The two halves are one line apart in the call graph.

`src/main.rs:1042-1045` runs the dispatch before the loop:

```rust
if let Some(path) = launch.open.clone() {
    let g = ui.global::<quire::UIState>();
    controller::import_from_path(&g, &state, &path);
}
```

`core/document.rs:124-133` is the per-change cost:

```rust
Change::BlockInserted(block) => {
    let vec = self.pages.entry(block.page).or_default();
    if vec.iter().all(|b| b.id != block.id) {   // O(n) every insert
        vec.push(block.clone());
        vec.sort_by_key(|b| b.order);           // O(n log n) every insert
    }
}
```

`import_markdown` (`core/services/import_service.rs:130-136`) builds all 100,001
`Change`s first — `PageCreated` then one `BlockInserted` per parsed line, each with an
`OrderKey::between(prev, None)` that is `b.0 + 1` (`core/types.rs:78`, O(1)) — and
`controller.rs:8656-8662` applies the whole vector through the loop above. So the
quadratic term is the dedupe scan and the re-sort, not the order keys and not the id
allocator (`document.rs:33-37` is a `next_id += 1`).

`state.record(rest)` (`controller.rs:8663`) and the store's WAL are the other two
per-block steps; this pass did not separate them, and it does not need to — the two
lines quoted above are enough to explain a curve that goes 2.6 → 27.5 → 128.

## Candidate fixes

1. **An append fast path in `apply`.** The vec is kept sorted by `order` and the
   incoming block's `order` is `prev + 1`, so a `push` after checking the last element's
   id needs no scan and no sort. `sort_by_key` on a vec that is already sorted except at
   the end is the single most expensive thing in this file for this workload.
2. **A bulk insert for a known-count batch.** `reserve_block_ids(n)` already exists
   (`document.rs:44-48`) and is what page duplication uses; `import_markdown` knows `n`
   before it starts and could take the same route: build the vec, sort it once, insert it
   once.
3. **Move the import behind the loop.** `--open` should queue a job the way
   `tools/call` does (`24`), so the window paints, the notice says "导入中", MCP and
   `--quit` and `--auto-exit` are all reachable while it works. This does not make the
   import fast; it makes the app alive while it is slow, which is the part the user
   feels.
4. **A cap, with a word.** `30` asked for the same thing on the `--pull` door. Both
   doors read an unbounded body into memory; only one of them has a person standing at
   the other end. A line count or byte count that refuses with a `set_db_notice`
   (`34`'s fix 2 is the same one-liner) is cheaper than any of the above.
5. **A gate with the curve.** `bigprobe.sh` and `cliprobe2.sh`'s `scale` arm already
   print the four numbers. The assertion to freeze is `100_000 lines → loop within N s`
   plus the control `1 line of the same bytes → loop within M s`, so a future
   "optimisation" that only touches the read cannot pass.

## Not measured

- **What the editor does when it has to show 100,000 blocks.** The arm opened the page
  (`open(g, state, new_id)`, `controller.rs:8664`) and the window was on screen for the
  three seconds before `--quit`, but this pass took no shot and made no claim about
  scroll, repaint or memory of the ListView. If there is a second defect behind this
  one, it is there and unmeasured.
- **The load path's 38 s** are attributed to the same `apply` loop by inference from the
  shape of the numbers; `repository.rs`'s read was not profiled.

## How this harness got it wrong

- The first `big` arm polled `console.log` for 60 s, gave up, printed `dumpstate_at=never`
  and then asked a blocked session to quit — which is where the refusal came from. The
  number looked like "the import never finished" when it meant "the import takes longer
  than the probe". `bigprobe.sh` exists to poll in seconds and print the elapsed ones.
- `cliprobe2.sh`'s `winof`/`alive` helpers first passed the pid as a PowerShell argument
  (`-Command "… $args[0] …" pid`), which PowerShell appends to the command instead of
  binding, so every helper returned the pid it was given and the first run of the curve
  has empty window columns. Interpolating the pid into the command fixed it — and the
  lesson is the general one: a probe that prints a plausible value for a call that never
  ran is worse than one that errors.
