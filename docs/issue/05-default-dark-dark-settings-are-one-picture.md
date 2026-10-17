# 05 · `default`, `dark` and `dark-settings` are one picture

**Severity: medium** — a corpus row that photographs the absence of a scene.

## Symptom

Three of the 186 PNGs share one hash: `default.png`, `dark.png`,
`dark-settings.png`.

## Cause, in two halves

**The sweep special-cases "default" into a no-scene render.**
`benchmarks/scripts/sweep.ps1:144`:

```powershell
$sceneArg = if ($s -eq "default") { @() } else { @("--scene", $s) }
```

so `default` runs `quire-shot` with **no** `--scene` at all. `apply_scene` is
never called, the theme stays at the built-in resolution (`Theme.slint:14-16`:
`""` → `midnight`), and the picture is the untouched shell.

**`dark` and `dark-settings` are the same untouched shell.**
`apply_scene` sets the theme from the name prefix (`controller.rs:9500-9502`,
`scene.starts_with("dark")` → `midnight`), then dispatches the body. `"dark"` has
its own arm (`controller.rs:9744`, `set_theme("midnight")`) which re-asserts what
the prefix already did. `dark-settings` has **no arm anywhere**:

```
grep -n 'dark-settings' src/app/controller.rs   →  (no match)
```

so it falls through to `_ => {}` and the shot is a bare midnight shell — the
same pixels as the other two.

The name was clearly meant to be the settings dialog in dark mode. The
`dark-*` overlay convention exists for exactly that
(`controller.rs:11586-11587` forwards `dark-<name>` to the light scene's
overlay), and `"dark-settings"` was never added to it.

## Reproduce

```
S=target-shot/debug/quire-shot.exe
$S --out a.bmp --w 1280 --h 800                      # what "default" gets
$S --out b.bmp --w 1280 --h 800 --scene dark
$S --out c.bmp --w 1280 --h 800 --scene dark-settings
# all three: identical md5
```

## Candidate fixes

- Add `"dark-settings" => apply_scene_overlay(ui, state, "settings")` beside the
  other `dark-*` forwards, plus its body arm — or, since `--theme` already
  exists on the shot binary, drop the theme-variant scene names in favour of
  `--scene settings --theme midnight`. The `--theme` flag was added for exactly
  this ("the one way to photograph a scene the catalog already seeds in a
  palette the scene's own name does not spell", `quire_shot.rs:383-388`).
- Decide what `default` is supposed to mean. If it means "the light shell", the
  sweep's no-`--scene` special case is wrong; if it means "whatever the app opens
  as", then `dark` is a duplicate of it and should be renamed. Either way the
  corpus should not contain two names for one state.
- The duplicate-hash gate from `04` covers this too.
