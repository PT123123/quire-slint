# 01 · sweep photographs a stale shot binary

**Severity: high** — the pixel gate can be green over a renderer that is not the
one in the tree.

## Symptom

`benchmarks/scripts/sweep.ps1:23` hardcodes

```powershell
$shot = "target\debug\quire-shot.exe"
```

and the script never builds it. Every recipe that produces that binary builds it
somewhere else: `justfile:60` (`just shot`) and `justfile:62` both pass
`--target-dir target-shot`, and `clips.ps1:3` defaults to
`target-shot\debug\quire-shot.exe`. ADR-0134 moved the software build out of
`target/` on purpose, and the sweep was left reading the old location.

`cargo check --workspace --all-targets` and `cargo test --workspace` — the two
lines that refresh `target/debug` — do **not** rebuild it either: `quire-shot` is
`required-features = ["software"]`, and the default feature set is `femtovg`, so
no current command writes `target/debug/quire-shot.exe` at all. Once ADR-0134
landed, that file became an artifact nothing refreshes.

## Reproduce

```
ls -l target/debug/quire-shot.exe target-shot/debug/quire-shot.exe
```

At the start of this pass the two were 85,921,792 B (`target-shot`) and a file
from **Oct 13** (`target/debug`) — 11 days behind a tree whose last commit is
later than both. `rc_gates.ps1` gate 4 calls `sweep.ps1`, so the RC gate inherits
the same stale path.

## Evidence

The 186-scene sweep run in this pass produced
`.scratch/issue-sweep-20261017/` — 186 PNGs, 0 `RENDER-FAIL`, 178 distinct
hashes — from a binary that predated the scenes it was asked to photograph. The
duplicate groups in `04`, `05` and `06` were all found *through* that run, so
they are real for the current code too (each was re-captured with
`target-shot/debug/quire-shot.exe` and reproduced), but a future pass cannot tell
from the log which binary drew which PNG: the sweep records no version, hash or
mtime of `$shot`.

## One write made by this pass

To stop the pass from judging an 11-day-old renderer, `target-shot`'s binary was
copied over the stale one:

```
cp -f target-shot/debug/quire-shot.exe target/debug/quire-shot.exe
```

Both files are now 85,921,792 B, Oct 17. No source file was touched.

## Candidate fixes

- Point `$shot` at `target-shot\debug\quire-shot.exe` to match every other
  consumer, and have `sweep.ps1` build it when missing (or refuse to run).
- Or add `-Shot` as a parameter the way `clips.ps1` already has, and pass
  `target-shot\debug\quire-shot.exe` from `rc_gates.ps1`.
- Stamp the run: print `$shot`'s `VersionInfo`/mtime/hash into `sweep.log`
  header, so a green sweep says *which* renderer produced it. This is the part
  that would have turned this into a red light rather than an accident.
- A gate in `rc_gates.ps1` that fails when `target/debug/quire-shot.exe` is
  older than the newest file under `src/` and `ui/` — the same freshness rule
  the installer checks already use.
