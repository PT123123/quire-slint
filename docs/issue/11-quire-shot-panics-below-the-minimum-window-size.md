# 11 · `quire-shot` panics below the minimum window size

**Severity: medium** — the responsive half of the UI cannot be photographed at
all, and the failure is a panic inside a dependency rather than a clean error.

## Symptom

```
quire-shot --out x.bmp --w 800 --h 600
  thread '<unnamed>' panicked at
  …/i-slint-renderer-software-1.18.0/lib.rs:591:9:
  buffer of size 480000 with 800 pixels per line is too small to handle a window of size 940x600
  Error: "render thread panicked"
```

No BMP is written. The same happens for any request whose width or height is
below the component's declared minimum.

## Bisection

```
940x600   → wrote x.bmp
939x600   → Error: "render thread panicked"
1280x500  → Error: "render thread panicked"
800x600   → Error: "render thread panicked"
```

The boundary is exactly `ui/AppWindow.slint:530-531`:

```slint
min-width: 940px;
min-height: 600px;
```

`--w`/`--h` set the *physical* window size (`ui.window().set_size(PhysicalSize::new(w, h))`,
`quire_shot.rs` `run()`), while the software renderer rasterises the window's
minimum-constrained logical size — 940×600 — into the buffer the caller asked
for. 800×600 = 480,000 px is smaller than 940×600, so Slint's own assertion fires
before any layout happens.

## Why this matters beyond a crash

- `benchmarks/scripts/sweep.ps1` sweeps 1280×800 (and whatever `-W/-H` are passed)
  and would abort the whole run, not skip a scene: the panic kills the render
  thread and `run()` returns `Err("render thread panicked")`.
- No narrow-window shot exists, so "what happens at 1000×620, 1100×700" — the two
  sizes where a two-pane shell usually starts to squeeze the editor column — are
  untestable, and the only responsive evidence this pass could gather is 1024×768
  (11 scenes, all re-lay out, none clip) and the three sizes in `06`.
- The app itself is safe (winit clamps to `min-width`), so this is a harness limit
  rather than a shipped defect — but it is a limit the harness does not announce.

## Candidate fixes

- Clamp in the binary before rendering: read the component's minimum, raise `w`/`h`
  to at least it, and print `note: requested 800x600 clamped to 940x600` so a
  sweep line never silently measures a different window than it asked for.
- Or fail cleanly: print `window size 800x600 below the declared minimum 940x600`
  and exit non-zero *before* the render thread starts, which is a message a script
  can grep instead of a panic in a registry source path.
- Add the narrow cases to the sweep as their own targets once the app's minimum is
  a decision rather than an accident: if 940×600 is the designed floor, then
  960×600 and 1024×640 are the shots worth having a baseline for, and `--w 940
  --h 600` is the boundary case that must not panic.
