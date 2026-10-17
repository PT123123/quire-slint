# 06 · `tasks-list-rename` renders no rename box

**Severity: medium** — a scene that documents a state the shot never shows. The
rendering half is *not* pinned; what is pinned is that the property write has no
effect on any pixel.

## Symptom

`tasks-list.png` and `tasks-list-rename.png` are byte-identical, at 1280×800,
1440×900 and 1680×950.

## Reproduce

```
S=target-shot/debug/quire-shot.exe
for wh in "1280 800" "1440 900" "1680 950"; do
  set -- $wh
  for s in tasks-list tasks-list-rename; do
    $S --out .scratch/issue-test/scr/$s-$1x$2.bmp --w $1 --h $2 --scene $s
  done
  node .scratch/issue-test/bmpdiff.js .scratch/issue-test/scr/tasks-list-$1x$2.bmp \
       .scratch/issue-test/scr/tasks-list-rename-$1x$2.bmp
done
# → diff_px=0 (0.000%) at all three sizes
```

Controls that show the diff method is not blind here:
`tasks-list` vs `tasks` = 6,754 px; `tasks-list` vs `tasks-list-menu` = 17,996
px; `tasks-list` vs `tasks-list-color` = 40,074 px. Same binary, same sizes.

## What the scene does

`src/app/controller.rs:9911-9920`:

```rust
"tasks-list-rename" => {
    let ids = org_scene_seed(state);
    org_scene_open(&g, state, 1);
    g.set_org_list(ids.lists[1]);
    g.set_org_list_renaming(ids.lists[1]);
    org_refresh(&g, state);
}
```

That is the `tasks-list` arm (`controller.rs:9841-9845`) plus the
`org-list-renaming` write — so the *only* intended difference is the rename.

## Why that difference should be visible

`ui/components/OrganizerArea.slint` binds the row from the same global
(`3043`):

```slint
renaming: !entry.smart && UIState.org-list-renaming == entry.id;
```

and `renaming` changes four things, all of them pixels: `nav-text.visible:
!root.renaming` (170), `count-text.visible: … && !root.renaming` (187), the ⋯
`visible: !root.renaming && …` (237), and `if root.renaming : Rectangle` (245) —
a 22 px box with `border-color: Colors.accent`, `background: Colors.background`
and a `TextInput` inside, which focuses itself in `init` (275-278). Hiding the
label and drawing an accent-bordered box cannot be zero pixels. So the binding
evaluated false.

## Not pinned — three candidates, with the experiment that settles each

1. **`ids.lists[1]` is not the id of a row in `task-list-rows`.**
   `org_scene_seed` (`controller.rs:7655-7657`) mints 工作/生活 through
   `state.org_create_list`, which is in-memory (`state.rs:9036-9059`, `exec_org`
   → `core::command::exec`), and the rows carry `id: l.id.0 as i32`
   (`state.rs:8226`). A mismatch here means the write is valid but matches
   nothing.
   *Settle it:* print the row ids and `org-list-renaming` after the arm runs.
   `--probe-blocks` (`quire_shot.rs:341-349`) is the existing shape of that door
   — a `--probe-org` that prints `org-list`, `org-list-renaming` and
   `task-list-rows[i].id` is ~10 lines and would end the guessing.
2. **The write is undone between `apply_scene` and the render.** `apply_scene`
   runs before `ui.show()`; `rebuild_organizer()` is what fills the model, and
   `set_org_list_renaming(-1)` exists at `controller.rs:6384` (the
   `org-list-name-set` callback) plus `UIState.org-list-renaming = -1` at
   `OrganizerArea.slint:3055/3057` (rename-committed / rename-cancelled). Neither
   is on this path as far as grep shows, but a Slint `init`/`changed` handler is
   not grep-visible.
   *Settle it:* the same probe, read twice — once before and once after the first
   render.
3. **`is-page`-style extra gate.** The sidebar twin binds
   `renaming: is-page && UIState.renaming-id == node.id`
   (`ui/components/SidebarItem.slint:31`), and the scene for it hardcodes
   `g.set_renaming_id(108)` (`controller.rs:10329`). If page 108 is not in the
   mock tree either, the two "rename" scenes are dead for the same reason and
   belong in one fix.
   *Settle it:* diff `--scene rename` against whatever its base scene is; a 0-diff
   pair means 108 does not exist in the fixture.

## Candidate fixes (after the probe says which)

- Seed the scene through the same door the ⋯ menu uses
  (`controller.rs:2290` `g.set_org_list_renaming(id)` is the live path) rather
  than writing the property from outside, so the scene cannot be more correct
  than the app.
- Add the duplicate-hash gate from `04` so this class of scene — a name whose
  pixels equal its base — fails the sweep instead of passing it.
