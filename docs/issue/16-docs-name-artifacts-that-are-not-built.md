# 16 · two documents describe the tree before the binary rename

**Severity: low** — no runtime effect; both mislead a reader who is reproducing a
number.

## README's reproduce command names a file that is not there

`README.md:36`:

> Reproduce with `benchmarks/scripts/bench_matrix.ps1 -Exe target\release\quire-slint.exe`

This pass, `target/release/` contains:

```
quire.exe            38,380,032 B  Oct 17 00:45   ← the main binary, under its old name
quire-typing.exe     37,610,496 B  Oct 17 00:45
quire_maxtoggle.exe  37,588,480 B  Oct 17 00:45
```

There is no `quire-slint.exe`. `Cargo.toml` now declares
`[[bin]] name = "quire-slint"`, so the name in the README is the *current* one and
the file on disk is the pre-rename leftover — a reader following the sentence
either gets "executable not found" or, worse, points `-Exe` at `quire.exe` and
benchmarks an 0.1.x binary from before the rename without being told.

Same class of drift as `02` (`rc_gates.ps1:63` reads the missing path and prints a
blank instead of failing).

## `Cargo.toml`'s binary comment undercounts

`Cargo.toml:22`:

```toml
# three binaries exist (quire / quire-shot / quire-typing); this is the app
default-run = "quire-slint"
```

`ls src/bin/` gives two files, and both are auto-discovered by cargo (only
`quire-shot` and `quire-typing` have explicit `[[bin]]` stanzas; `quire_maxtoggle`
is picked up by the default `src/bin/*.rs` rule):

```
src/bin/quire_maxtoggle.rs
src/bin/quire_shot.rs
src/bin/quire_typing.rs
```

plus `src/main.rs` as the app = **four** binaries. The comment's own list is also
stale in its first entry: it names `quire`, which is the pre-rename spelling it is
supposed to have been updated for.

`quire_maxtoggle` has no consumer: `grep -rn maxtoggle --include=*.ps1 --include=
justfile --include=*.md` returns one hit, `docs/DECISIONS.md:8149`, and that line
spells the binary `quire-maxtoggle` with a hyphen while cargo builds it as
`quire_maxtoggle.exe` — so the command in the ADR does not run either.

## Candidate fixes

- One commit, no code: README:36 keeps the current name (it is right) and the tree
  gets a release build so the file exists — or the sentence names the recipe
  (`just release-build` then `bench_matrix.ps1 -Exe target\release\quire-slint.exe`)
  so the reader is told to build first.
- Delete or overwrite the leftover `target/release/quire.exe` when the renamed
  binary is built (a `justfile` line, not a manual step — `install/sweep.ps1`
  already exists for exactly this kind of stale-artifact cleanup).
- Fix the comment to name all four and say which of them ship: `quire-slint` (the
  app), `quire-shot` (`required-features = ["software"]`, visual regression),
  `quire-typing` (scene E bench), `quire_maxtoggle` (a probe binary — and if it is
  no longer used, deleting it is the better fix than documenting it).
