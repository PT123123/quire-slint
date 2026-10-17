# 07 · interaction shots never pump the animation clock

**Severity: high** — every `--click` / `--key` / `--hover` shot, and therefore the
popup half of the sweep, captures all 60 `animate` blocks at their pre-write
value. This is not "a frame mid-tween"; it is systematically the frame *before*
the tween.

## The two halves of the harness are not equal

`src/bin/quire_shot.rs` has one path that owns the clock and one that does not.

**Motion path** (`capture_motion`, used by `--motion` and `just clips`) — advances
virtual time and publishes it:

```rust
CLOCK.with(|c| c.set(Some((k as u64 + 1) * frame_ms as u64)));
controller::apply_motion(ui, state, motion, k, frames);
slint::platform::update_timers_and_animations();
```

**Interaction path** (`quire_shot.rs:480-492`, used after `--hover`, `--click`,
`--key`) — redraws only:

```rust
window.request_redraw();
window.draw_if_needed(|renderer| { … renderer.render(…); … });
```

No `CLOCK` write, no `update_timers_and_animations()`. And `CLOCK` is armed only
for a motion capture (`run()`: `if motion.is_some() { CLOCK.set(Some(0)) }`), so
on the interaction path `HeadlessPlatform::duration_since_start()` falls through
to the wall clock. The gap between the property write and the redraw is the time
the software rasteriser takes — microseconds to milliseconds — against tweens of
80 / 140 / 220 / 320 ms (`ui/Theme.slint:51-54`). Even if the tick were read, the
tween would be at ~0 % of its progress.

## Evidence that the route works and the animation is what is missing

`ui/components/SidebarItem.slint` reacts to hover in two ways:

```slint
52:  : (ta.has-hover && !renaming ? Colors.hover-fill : transparent);
53:  animate background { duration: Theme.duration-instant; }     // 80 ms
88:  visible: !chev-ta.has-hover;                                  // the icon swap
98:  visible: chev-ta.has-hover || node.icon == "";
```

```
quire-shot --out a.bmp --w 1280 --h 800
quire-shot --out b.bmp --w 1280 --h 800 --hover 120,180
bmpdiff a b  →  diff_px=262  (0.026%)  bbox x=18..57 y=163..174  (40×12)
```

The 40×12 box is the chevron/icon slot — the **non-animated** `visible` binding
(88/98) renders. The row-wide hover fill (52/53), which spans the whole sidebar
row, contributes nothing: the tween is invisible. Controls: the same file against
itself is 0 px, and `--theme light` against `--theme midnight` is 99.997 % — so
the diff method sees real changes.

Editor side, the same asymmetry comes out as a flat zero: hovering 12 positions
across the block gutter and first column (`--scene edit`, x ∈ {330, 360, 390, 420}
× y ∈ {170, 210, 250}) changes **0 pixels in all 12**, and
`ui/components/BlockHandle.slint:19-20` is

```slint
opacity: visible-flag ? 1 : 0;
animate opacity { duration: Theme.duration-fast; }        // 140 ms
```

so the ⋮⋮ handle — the single most-photographed hover affordance in a
Notion-like editor — cannot be captured by this harness at all.

## Consequences

- The sweep's PNGs under-represent every arrival: a popup that fades/scales in is
  photographed at its start value, and `diffbbox` judges a picture the app never
  shows at that moment.
- `just clips` is the only place the animation machinery is exercised, and it
  covers 4 motions (`08`).
- The interaction shots that *were* used to prove Escape and outside-click
  behaviour (`09`, `10`) are valid — closing a popup is a `visible`/`is-open`
  change, not a tween — but a "no pixels moved" result there can also mean
  "a tween sat at frame 0", which is why the `dialog` 0-diff is not evidence.

## Candidate fixes

- Give the interaction path the same instrument as the motion path: after
  dispatching the event, set `CLOCK` to a value past the longest duration
  (`duration-slow` = 320 ms, so ~400 ms), call
  `slint::platform::update_timers_and_animations()`, then redraw. That makes
  every interaction shot a *settled* shot, which is what a baseline wants.
- Or make it explicit: add `--settle <ms>` to the shot binary and have
  `sweep.ps1` pass `--settle 400`, so the corpus states which tick it photographed
  instead of defaulting to frame 0.
- Either way, record the tick in the PNG's caption/log line, the same way
  `renderer_name` was fixed to say "Software · headless" rather than the
  compile-time guess.
