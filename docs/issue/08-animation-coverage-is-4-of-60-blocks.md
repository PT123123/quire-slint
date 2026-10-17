# 08 · animation coverage is 4 of 60 blocks, and 15 components have none

**Severity: medium** — two different gaps that show up as the same missing
coverage: one in the harness, one in the UI.

## Census

```
grep -rho "animate [a-z-]*" ui/ | wc -l          → 60
grep -rl "animate " ui/ | wc -l                  → 22 of 44 .slint files
```

`apply_motion` (`src/app/controller.rs:9524-9590`) implements exactly four names
and falls through to `_ => {}` for anything else:

- `sidebar-toggle`
- `theme-fade`
- `mcp-toggle`
- `board-drag`

`benchmarks/scripts/clips.ps1` gates exactly those four, with distinct-frame
floors `sidebar-toggle 5`, `theme-fade 4`, `board-drag 5`, `mcp-toggle 4`. A run
of `just clips` this pass: **4/4 pass**.

So: 60 `animate` declarations exist, 4 are provably interpolating, 56 are
unmeasured. Passing `--motion anything-else` to the shot binary silently produces
a clip of a static UI (the `_ => {}` arm writes no property), and `clips.ps1`
would report `JUMPED (no tween)` only if it were asked to run that name at all.

## The 15 components with no `animate` at all

Token and theme files aside (`Colors.slint`, `Theme.slint`, `Typography.slint`,
`Icons.slint`, `Types.slint`, `TrayIcon.slint`), these UI components contain zero
`animate` blocks:

```
AppShell  BacklinkPanel  ColumnsBlock  CommandPalette  DatabaseCell
DatabaseFilterPopup  DatabaseFormulaPopup  DatabaseRelationPopup
DatabaseRollupPopup  DatabaseView  Dialog  Editor  PageTree
QuickNoteWindow  SearchPanel  TableBlock
```

(`for f in ui/*.slint ui/components/*.slint; do grep -q "animate " $f || echo $f; done`)

Every one of those is either an overlay or a container whose *contents* arrive by
being added to the tree — so `CommandPalette`, `SearchPanel`, `Dialog`,
`BacklinkPanel`, the four Database popups and `QuickNoteWindow` open with no
transition at all. That is visible in the interaction shots: `palette + Escape`
moves 230,067 px in one step (22.5 % of the window appearing/disappearing between
two adjacent frames). Whether this is a defect is a design call — SPEC's rule is
"有反馈但不持续运行", and `Theme.slint` publishes four durations that suggest
everything is meant to move — but it is currently *unmeasured either way*, because
`apply_motion` has no arm for any of them.

## Candidate fixes

- Make the harness honest first: have `--motion <name>` fail loudly when
  `apply_motion` has no arm for it, instead of rendering 20 identical frames of a
  scene that never moved.
- Add motions for the arrivals that have durations available already — a popup's
  `opacity`/`scale` tween would need the components above to *have* one, so this
  splits into (a) 56 measured-but-existing blocks, and (b) a design decision about
  the 15 components that have none.
- (a) is cheap: the census is a script (`grep -c animate` per file, plus the list
  of implemented motion names from `apply_motion`), so it can be a printed line in
  `clips.ps1`'s output — "4 of 60 animate blocks measured" — which turns silent
  drift into a number that goes up.
- If the answer for the overlays is "they should pop" (a defensible Notion-like
  choice: menus appear instantly), say so in `docs/UI_ARCHITECTURE.md` next to the
  duration tokens, so the next pass does not re-open it.
