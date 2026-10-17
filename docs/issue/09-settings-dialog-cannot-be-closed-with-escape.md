# 09 · the settings dialog has no working Escape

**Severity: high** — a full-window panel that the keyboard cannot close, and that
does not stop a click from reaching the UI under it.

## Evidence

```
quire-shot --out a.bmp --w 1280 --h 800 --scene settings
quire-shot --out b.bmp --w 1280 --h 800 --scene settings --key escape
bmpdiff a b  →  diff_px=0  (0.000%)
```

Positive controls on the same binary and the same key event:

| scene | Escape diff |
|-------|------------:|
| `palette` | 230,067 px — *something* changed |
| `slash` | 127,648 px (bbox x=340..619 y=260..715) — *something* changed |
| `icon-picker` | 98,672 px — *something* changed |
| `menu` | 62,895 px (bbox x=240..423 y=438..779) — *something* changed |
| `settings` | **0 px** |
| `dialog` | 0 px — by design: `Dialog.slint:13` sets `close-policy: PopupClosePolicy.no-auto-close` |
| `link-dlg` | 0 px — same `no-auto-close`, `AppWindow.slint:428` |
| `database-filter`, `database-formula`, `page-versions` | 0 px — no readable cause, see `10` |

None of the four non-zero rows proves a close — `10` explains why a pixel change is
not a dismissal with this harness. They are here as positive controls: the key
event lands and the app repaints. The other 0 px rows are either a documented
`no-auto-close` (`dialog`, `link-dlg`) or still unexplained (`10`); `settings` is
the one this record can account for from the source, below.

`settings` is in the group that *can* be tested: `SettingsDialog` is not a
`PopupWindow`.

## Cause

`ui/components/SettingsDialog.slint:333`:

```slint
export component SettingsDialog inherits Rectangle {
    width: 100%;
    height: 100%;
```

so it is an app-driven overlay placed by `ui/components/AppShell.slint:75`
(`if UIState.settings-open : SettingsDialog {}`), not a backend-managed popup —
no automatic dismiss, and by design no scrim.

The panel's only Escape door is on the filter field
(`SettingsDialog.slint:407-412`):

```slint
key-pressed(event) => {
    if (event.text == Key.Escape) {
        UIState.settings-close-requested();
        accept
    } …
}
```

A `TextInput`'s `key-pressed` only fires when it has focus. The scene — and the
app on opening the panel — never focuses it, so Escape goes to the window, nobody
handles it, and the panel stays. The mouse door is the 完成 button
(`SettingsDialog.slint:743-746`).

Meanwhile the app already has the right pattern elsewhere: the notes浮层 uses a
window-level shortcut (`ui/AppWindow.slint:939-945`, `keys: @keys(Escape)`,
"Escape puts the notes half's note 浮层 down (ADR-0118) — the same ask its ✕
makes. Guarded on the panel actually being up"). Settings has no such shortcut.

## Second half: the overlay is not input-modal

```
quire-shot --w 1280 --h 800 --scene settings --click 30,30
  before-input: … palette-open=false …
  after-input:  … menu-open=true        ← flipped
bmpdiff vs base → 12,478 px, bbox x=260..443 y=48..115
```

A click at the window's top-left, over a panel that paints a full-window plate
(`background: Colors.page` at 100 %×100 %), opened the app menu. A `Rectangle`
without a `TouchArea` does not consume input in Slint, so everything under the
settings plate is still clickable. Two possible readings — the plate is hit-test
transparent, or a control above it took the click — and the harness cannot
distinguish them because `probe()` does not print `settings-open` (`10`). Either
way, the panel is not modal to the pointer while it is modal-looking.

## Candidate fixes

- Add the ADR-0118 shape to settings: a `Shortcut { keys: @keys(Escape);
  if UIState.settings-open { … } }` at the window level, calling the same
  `settings-close-requested()` the 完成 button calls. One block, no new property.
- Or focus the filter field when the panel opens (the rename box already does
  this with `rename.focus()` in `init`, `OrganizerArea.slint:275-278`) — but that
  makes Escape work only while the field has focus, which is not what a user
  expects from a panel-level key.
- For the modal question, decide it explicitly: either give the overlay a full
  `TouchArea` that swallows (or closes on) clicks, or document that settings is a
  non-blocking panel. `UIState.settings-open` is also what the click-through
  experiment could not verify — see `10`'s probe gap.
- Add `--scene settings --key escape` → "diff must be > 0" to the interaction
  tests once fixed, so the door cannot silently lose its handler again.
