# 04 · the three sync scenes photograph the 外观 tab

**Severity: high** — six scenes claim to document a panel they never show.

## Symptom

`settings.png`, `sync.png`, `sync-stats.png` and `sync-log.png` are
**byte-identical** (same md5). The same four-way collision repeats in dark:
`dark-sync`, `dark-sync-stats`, `dark-sync-log` are identical to each other.

## Reproduce

```
S=target-shot/debug/quire-shot.exe
for s in settings sync sync-stats sync-log; do
  $S --out .scratch/issue-test/$s.bmp --w 1280 --h 800 --scene $s
done
# then compare the PNGs' hashes, or diff any pair with bmpdiff
```

Result this pass: one hash across all four; 0 px difference.

## Cause

`src/app/controller.rs:9819-9836`, the `"sync" | "sync-stats" | "sync-log"` arm:

```rust
sync_scene_seed(state, scene);
g.set_settings_open(true);
if scene == "sync-stats" { g.set_sync_tab(1); }
if scene == "sync-log"   { g.set_sync_tab(2); }
refresh_sync_ui(&g, state, &Default::default());
```

It opens the settings dialog and moves the **inner** sync segment (`sync-tab`)
but never navigates the dialog itself to the 同步 tab, which is
`UIState.settings-tab == 2`. The dialog's tab array is
`["外观","通用","同步","MCP","快捷键","关于"]`
(`ui/components/SettingsDialog.slint:300`) and the sync section is gated on
`UIState.settings-tab == 2` (`SettingsDialog.slint:616-630`). With
`settings-tab` left at 0, all three sync scenes render the 外观 tab — the same
pixels `settings` renders — and the seeded peers table, its stats panel and its
log are all off-screen.

`set_settings_tab` is never called anywhere in the scene arms (grep of
`src/app/controller.rs` for the setter inside `apply_scene_body`).

## Candidate fixes

- Add `g.set_settings_tab(2);` to the sync arm, next to `set_settings_open`.
  One line, and it makes six scenes (three light, three dark) photograph the
  panel they are named after.
- Extend `sync_scene_seed`'s own comment: it already insists the table be filled
  by the real `sync_note_device` door — the tab switch belongs to the same rule
  (the 同步 tab's own click handler is `settings-tab = 2`).
- Add a duplicate-hash check to `sweep.ps1`: compare each new PNG's hash against
  the rest of the run and print the groups. The check is ~10 lines of
  `Get-FileHash` + `Group-Object`, and it turns a silent collision into a
  reported one. This pass found 4 such groups covering 12 of the 186 files
  (`04`, `05`, `06`), so a hash-group gate would have surfaced all three at once.
