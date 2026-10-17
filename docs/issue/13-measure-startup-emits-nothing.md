# 13 · `--measure-startup` prints nothing

**Severity: high** — the flag the startup numbers in README/PERFORMANCE.md are
produced by emits no record on either profile, on the default renderer.

## Reproduce

```
./target/debug/quire-slint.exe   --db .scratch/issue-test/ms.db  --measure-startup --auto-exit 5 2>ms.err
./target/release/quire.exe       --db .scratch/issue-test/msr.db --measure-startup --auto-exit 6 2>msr.err
grep -c first_paint ms.err msr.err        # → 0 and 0
cat ms.err                                 # → "quire: mcp server on 127.0.0.1:5880" and nothing else
```

Both exit 0, both open and draw a window, neither prints a `first_paint` JSON
line. The `[measure]` diagnostic branch is also silent, which is the informative
half: `src/main.rs:425-446` only writes that line when
`set_rendering_notifier(...)` returns `Err`, and the fallback 1 ms
`event_loop_proxy` timer is started inside that `is_err()` branch. So the
notifier was **accepted** and never called back — the one state with no output.

## What is supposed to happen

`install_startup_measurement` (`src/main.rs:379-448`), doc comment:

> Slint 1.18 exposes `Window::set_rendering_notifier`; **femtovg and skia support
> it and fire `AfterRendering` once per drawn frame**, so the first firing is the
> first painted frame…

The installed closure prints:

```rust
{{"event":"first_paint","renderer":"{…}","first_paint_ms":{:.1},"method":"AfterRendering"{phases}}}
```

The comment already documents one silent case — a `--features skia` (wgpu/
softbuffer) build returns `Ok(())` and never fires, with the instruction "never
read a silent skia run as 'no frame was drawn'". This pass was **not** a skia
build: the default feature set is `femtovg` (`Cargo.toml`, `default = ["femtovg"]`),
which the same paragraph claims does fire. So either that claim is now false for
1.18 + winit's software/GDI path, or the femtovg renderer draws its first frame
before the notifier is installed — the comment explicitly rules that out ("The
window is still hidden at install time, so no frame can be missed", and the call
site is `main.rs:620`, immediately after `AppWindow::new()` and before `ui.show()`).

## Blast radius

`benchmarks/scripts/startup_bench.ps1` and the `startup ms` column of
README's Measured table parse this stderr line. A run today yields no number — so
a milestone table refreshed against the current tree either used a *different*
mechanism (the `PhaseLog` phases come out only inside the same missing line, so
they are lost too) or the number is stale. README:36's reproduce command points at
`target\release\quire-slint.exe`, which is `16`.

## Candidate fixes

- Add the third case: fire a one-shot check after the first `AfterRendering`
  *or* after N event-loop iterations, and if the notifier never fired, print the
  proxy record with `"method":"notifier_silent"`. A measurement that is missing is
  worse than a measurement that is labelled low-confidence, because the table
  cannot tell them apart.
- Print something on `--measure-startup` **unconditionally** at the point the
  window is shown — even `{"event":"first_paint","method":"none"}` — so the
  bench script's parse either succeeds or sees an explicit failure.
- Re-check the claim against the pinned Slint: a 10-line `examples/` probe with
  `set_rendering_notifier` on `renderer-femtovg` + winit would say whether
  `AfterRendering` fires at all under this configuration, and the answer belongs in
  the doc comment either way.
