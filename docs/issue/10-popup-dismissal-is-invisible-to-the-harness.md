# 10 · ten Escape tests, four readable outcomes, and the harness cannot tell you which is which

**Severity: medium** — the dismissal test gives a clean 0 px on six scenes, and
only three of those have a cause readable from the code. The four scenes that *do*
move pixels are no better off: "the picture changed" is not "the popup closed". The
rest need a probe the shot binary does not have.

## Measured (`--key escape` against the same scene without it, 1280×800)

| scene | overlay | `close-policy` | Escape diff |
|-------|---------|----------------|------------:|
| `palette` | `CommandPalette` PopupWindow | close-on-click-outside | 230,067 px — *something* changed |
| `slash` | slash command list | close-on-click-outside | 127,648 px — *something* changed |
| `menu` | ContextMenu | close-on-click-outside | 62,895 px — *something* changed |
| `icon-picker` | `IconPicker` PopupWindow | (self-managed) | 98,672 px — *something* changed |
| `settings` | `SettingsDialog` **Rectangle** | n/a | **0 px** — see `09` |
| `dialog` | `ConfirmDialog` PopupWindow | **`no-auto-close`** (`Dialog.slint:13`) | 0 px — by design |
| `link-dlg` | `LinkDialog` (AppWindow) | **`no-auto-close`** (`AppWindow.slint:428`) | 0 px — by design |
| `database-filter` | `DatabaseFilterPopup` | close-on-click-outside | 0 px — unexplained |
| `database-formula` | `DatabaseFormulaPopup` | close-on-click-outside | 0 px — unexplained |
| `page-versions` | `VersionPanel` | close-on-click-outside | 0 px — unexplained |

Three of these rows are now readable from the source: the two `no-auto-close`
popups are *supposed* to ignore Escape and the harness measured exactly that (this
is why the earlier reading of them as "dismissal works, test can't see it" was
wrong and is retracted here). `settings` is `09`. What is left is the last three
rows: three popups that ask for click-outside dismissal, self-focus an input in
`init` (`DatabaseFormulaPopup.slint:127`, `VersionPanel.slint:44`), carry an
`Escape` branch on that input — and still move 0 pixels.

`slash`, `menu` and `icon-picker` deserve the opposite caution: a large diff is not
proof of dismissal. `IconPicker.slint` has neither a `Key.Escape` branch nor a
`focus()` call, so the likeliest reading is that Escape moved *focus* somewhere else
in the window and the picture changed without the picker closing. `palette` is the
one row with a self-focus door in the code (`CommandPalette.slint:24`), so its
230,067 px is the best-supported of the three, but it is the same kind of inference.
None of the four can be stated as "closes", because the diff cannot say which, for
the reason below.

## Why the harness cannot settle it: `probe()` prints 7 flags of 24

`ui/Types.slint` holds 24 `property <bool> …-open` — the exact state every one of
these rows turns on:

```
sidebar-open slash-open link-open find-open db-columns-open db-filter-open
db-group-open db-search-open palette-open search-open menu-open
icon-picker-open versions-open dialog-open settings-open db-formula-open
db-relation-open db-rollup-open org-note-search-open org-note-sort-open
org-note-details-open org-bin-open org-cmd-open org-capture-open
```

`quire_shot.rs`'s `probe()` prints `menu-open, slash-open, slash-insert,
block-menu-id, palette-open, search-open, capture-open` — seven, none of them
`icon-picker-open`, `dialog-open`, `versions-open`, `settings-open` or any
`db-*`/`org-*`. So each of the nine rows above is a pixel count with no state line
beside it: "the picture changed" and "the popup closed" are the same observation,
and "nothing changed" and "the harness could not deliver the gesture" are the same
observation too. The `icon-picker` and three-unexplained rows are exactly the
questions a two-line probe would answer.

## Candidate fixes

- Print every `*-open` flag in `probe()` plus the non-boolean properties the scene
  arms write (`settings-tab`, `sync-tab`, `org-list`, `org-list-renaming`,
  `icon-picker-node-id`). Cheap, and it converts `09`/`06`/`04` from pixel
  inference into assertions.
- Then re-run the nine rows. Expected outcome: `dialog`/`link-dlg` 0 px with
  `dialog-open=true link-open=true` (design), `settings` 0 px with
  `settings-open=true` (the `09` fix), and the three database/version rows either
  confirming a dead Escape branch or showing the popup was never up in the shot —
  a third possibility the pixel test alone cannot exclude.
- Give the headless platform an explicit dismissal door for
  `close-on-click-outside`: a `--dismiss` flag that calls the same close callback
  the backend would, so "does this popup close" stops depending on whether the
  software window emulates a native popup.
- Add the `no-auto-close` pair to the interaction corpus as a **negative** control
  (`--key escape` must be 0 px) — a gate that can fail in both directions is the
  only way a 0 px row means something.
