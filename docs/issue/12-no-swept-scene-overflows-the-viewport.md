# 12 · no swept scene ever overflows the viewport

**Severity: medium** — a knob exists, is documented as self-checking, and the
corpus cannot exercise it; scrolled rendering is therefore unbaselined.

## Symptom

`quire-shot --scroll-y <v>` sets `UIState.editor-scroll-y` (`quire_shot.rs:407`,
declared `in-out property <length> editor-scroll-y: 0px; // Editor viewport
mirror` at `ui/Types.slint:654`, two-way bound to the Flickable at
`ui/components/Editor.slint:407`: `content-y <=> UIState.editor-scroll-y`).

The binary's own comment states the acceptance rule:

> `--scroll-y` drives the same viewport mirror scene F uses. It exists because a
> scroll that moves nothing still burns CPU, and the bench harness shipped with
> that bug: **a run at 0 and a run at ±2000 have to give different pixels before
> either number means anything.**

Measured at 1280×800, that rule fails on every scene tried:

```
scene            diff(scroll-y 0 → 2000)
edit             0 px
marks            0 px
table            0 px
code-hl          0 px
backlinks        0 px
database-table   0 px
tasks            0 px
notes            0 px
settings         0 px
page-versions    0 px
image            0 px
columns          0 px
```

## It is the fixture, not the knob

At `--h 600` (the smallest window the component allows, see `11`) the same
property moves real content:

```
scene            diff(0 → 400) at 1280x600     bbox
marks            39,700 px  (5.169 %)          x=390..1275 y=1..599
table            28,946 px  (3.769 %)          same
code-hl         203,413 px  (26.486 %)          same
backlinks        29,300 px  (3.815 %)          same
page-versions    13,902 px  (1.810 %)          same
edit                  0 px                     (page fits 600 too)
```

The bbox starting at x=390 is the editor column, and it is the only thing that
moves — the sidebar and header stay put, which is the correct scroll behaviour.

So every fixture page in the corpus is shorter than 800 px of content. Consequent-
ly the sweep has never photographed: a block clipped at the top edge, the sticky
header over scrolled content, virtualisation recycling rows mid-scroll, or the
scrolled-state rendering that `DatabaseView.slint:98` computes from
(`top-in-view: root.row-y - UIState.editor-scroll-y`).

Also: `grep -rn "scroll-y" benchmarks/scripts/*.ps1` returns **nothing**. The flag
has no consumer in any script, so nothing would notice if it broke.

## Candidate fixes

- Give the sweep one long fixture: a scene whose page is ≥ 3× the viewport height
  (`--scene long-page`), swept at `--scroll-y 0`, `1200`, `2400` — three rows of
  the same page at three depths. That is the cheapest way to put the scrolled
  states under a baseline.
- Make the self-check the comment promises into a gate: in `sweep.ps1`, when a
  scene is run with a `--scroll-y` variant, assert the pair differs, and fail the
  run if it does not. This pass proved that assertion fails today, which is the
  proof it is worth having.
- Note the interaction with `07`: a scroll *inertia* animation, if one is ever
  added, will be invisible to the same interaction path that hides hover fades.
