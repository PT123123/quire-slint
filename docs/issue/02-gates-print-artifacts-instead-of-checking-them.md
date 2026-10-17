# 02 · gates print artifacts instead of checking them

**Severity: medium** — a missing artifact reads as a number, not a failure.

## Symptom

`benchmarks/scripts/rc_gates.ps1:63` (release gate):

```powershell
$exe = Get-Item "target/release/quire-slint.exe" -ErrorAction SilentlyContinue
Record "release" (($code -eq 0) -and ($warns -eq 0)) "exit $code, $warns warning lines, exe $($exe.Length) B"
```

`$exe.Length` is only *interpolated into the note*. The gate's verdict is
`exit 0 and no warnings`, so an absent exe records **PASS** with a blank size.
The same shape appears in gate 8 (`dist`), which checks `$null -ne $zip` — that
one does fail correctly; the release gate is the one that only prints.

Measured: with the current tree,

```
$ exe = Get-Item 'target/release/quire-slint.exe' -EA SilentlyContinue
$ "exit 0, 0 warning lines, exe $($exe.Length) B"
  exit 0, 0 warning lines, exe  B
```

## Why the file is missing right now

`target/release/` holds `quire.exe`, `quire-typing.exe`, `quire_maxtoggle.exe`,
all Oct 17 00:45 — the **pre-rename** main binary. `Cargo.toml` now declares
`[[bin]] name = "quire-slint"`, so a release build writes `quire-slint.exe` and
nothing removed `quire.exe`. Gate 3 runs `cargo build --workspace --release`
itself, so a *complete* gate run does create the new name — the point is not that
the file can never exist, it is that the gate's verdict ignores whether it does.
Any pass that reads a tree at this state (and `-SkipInstallers` passes that skip
`just dist`) records "release: PASS … exe  B" over a missing artifact.

Worse, a stale `target/release/quire.exe` is what the pre-rename scripts would
have found; the name now used by `install/build-installer.ps1:18`,
`install/deploy-workshop.ps1:100`, `install/release-publish.ps1:97` and
`install/verify-installer.ps1:59` is `quire-slint.exe`, so a tree that has only
the old name cannot silently satisfy the installer — those scripts do fail. The
gap is specifically the *report* the release gate writes.

## Candidate fixes

- Make the release gate assert: `$null -ne $exe` in the verdict expression, plus
  a floor (`$exe.Length -gt 20MB`) so an empty or truncated link is not green.
- Have `rc_gates.ps1` fail on a leftover `target/release/quire.exe` (or delete
  it in the same step that builds the renamed exe) so the tree cannot hold two
  main binaries.
- Print the exe's `VersionInfo.FileVersion` next to the size: the gate then
  answers "which build did I just check" instead of "how big is some file".
