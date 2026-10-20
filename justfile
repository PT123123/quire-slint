set shell := ["powershell.exe", "-NoProfile", "-Command"]

# list available recipes
default:
    @just --list

# daily build: dev profile, incremental, only the application binary
build:
    cargo build --bin quire-slint

# release build (default FemtoVG renderer; use for packaging and deployment)
release-build:
    cargo build --release --bin quire-slint

# run in debug; pass cargo flags as one quoted string
# e.g. just run "--no-default-features --features skia"
run *args:
    cargo run {{ args }}

# local CI replacement (the GitHub workflow was removed on purpose):
# everything a push would run, before you commit.
# `--workspace` survived the two-crate week and is now decoration: this workspace
# has one member again, and `quire-core` is a *dependency*, which no cargo flag
# run from here will test. Its 431 tests belong to that repository
# (`cargo test --all-targets` there). So a green `just check` here means the shell
# compiles against the pinned rev and its own suite passes — never quote it as
# "the whole app is green".
#
# The third line keeps `--release` and is *not* to be weakened: measured
# 2026-10-05 at `72a69f9`, replacing it with a release-opt-in dev-profile build
# (`--profile dev` + `lto`/`codegen-units`/`strip` via `--config`) rebuilt **325
# crates** — switching a profile invalidates every unit in the graph, the same
# 7m59s trap PERFORMANCE.md records for `codegen-units`, and it would have made
# every `just check` slower rather than faster. `--profile` is also mutually
# exclusive with `--release`, so there is no flag that asks for "release checks,
# cheap". The line stays exactly as it was. `docs/BUILD_PERFORMANCE.md` §2 has
# the numbers (a `.rs` touch is 209 s: 11 s link, ~198 s one crate's codegen at
# `codegen-units = 1`) and §4-R1 why that is a structural cost rather than a
# misconfiguration.
check:
    cargo check --workspace --all-targets
    cargo test --workspace
    cargo build --workspace --release

# headless visual shot: software-rendered PNG of the real UI, no window.
# Scene names: default dark palette search-notes menu rename settings dialog empty
#
# The separate `--target-dir` is the point of this recipe (ADR-0134). `software`
# is a *different feature set* from the default `femtovg`, so cargo sees the two
# builds as unrelated units: sharing one target directory meant it kept every
# unit it had ever produced for this crate, and because the app's rlib is ~1 GB
# with debug info, alternating between `just run` and `just shot` accumulated 26
# copies of the same library — 63 GB of the shell's 71. `target-shot` is its own
# directory, so a shot no longer invalidates the app build and the two do not
# compete for the same space; `just clean` removes it.
shot scene="default":
    cargo build --features software --bin quire-shot --target-dir target-shot
    .\target-shot\debug\quire-shot.exe --out .scratch\shots\latest.bmp --scene {{ scene }}
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\shot2png.ps1

# The README's clips: one headless motion capture per animation, into docs\clips.
# A clip that did not interpolate is a GIF of a jump, so the recipe fails the
# run when a motion's distinct-frame count is under its own floor.
clips:
    cargo build --features software --bin quire-shot --target-dir target-shot
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\clips.ps1 -Shot target-shot\debug\quire-shot.exe -GifW 320 -Frames 14

# How many frames does the sidebar's 220 ms drawer tween actually paint?
#
# `clips` answers "did it interpolate" from a headless capture; this answers
# "how many frames did the real window put on the screen", which is the number
# behind "动画不够顺畅，像是帧数比较少" (ADR-0168). It needs a real window on a
# real monitor, so it runs the app itself with `--frame-cadence` against a copy of
# this machine's library, runs it three times and reports the median per arm — a
# single run on this machine spreads from 8 to 17 frames for the same arm. The worst
# arm failing under 6 frames is the gate; 6 is deliberately *below* today's measured
# spread, because a floor inside the noise gets ignored. A GIF of a jump and a
# 1-frame drawer both look fine until someone counts the frames.
cadence:
    cargo build --bin quire-slint
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\frame_cadence.ps1

# the memory and leak gates (ADR-0169): the two bounds as tests, the per-event
# Timer ratchet, and the Private-byte floor of an app on a copy of this
# machine's library — each arm paired with the control that proves it can fail.
# ~4 min with the drift arm; `... -SkipDrift` for the two cheap arms. The drift
# probe runs beside a Quire that is already open (`--auto-exit` + `--db` is the
# harness pairing, ADR-0169), so the gate is not an "app must be closed first" job.
leakgate:
    cargo build --bin quire-slint
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\leak_gate.ps1

# package the release exe into a zip (M8-lite; installer comes later)
dist:
    cargo build --release --bin quire-slint
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\dist.ps1

# deploy a release into the workshop's archive: one folder per release,
# C:\workshop\quire-slint-<version>\quire-slint-<version>.exe, so the file
# carries the version the folder is named after. The name is `quire-slint`, the
# shell: the workshop lists one folder per release of several applications, and a
# bare `quire-<version>` would not say which of the two shells put it there. The
# script bumps [package] version's patch, builds, and commits and
# pushes the bump; the archive folder is new every release, so nothing is ever
# overwritten and no running instance is ever in the way (ADR-0139). The bump
# has to precede the build because build.rs stamps the exe's version block from
# that same key.
#
# **This is the default deploy, and it builds the *dev* profile** (decided
# 2026-10-20: a workshop deploy is asked for when somebody wants a current exe to
# run, not when the archived bytes have to be the audited size). Identical
# ceremony to `deploy-workshop-release` — bump the patch, commit + push the bump,
# archive to C:\workshop\quire-slint-<version>\ — and identical destination. Only
# the build differs: `cargo build --bin quire-slint` (dev) instead of `--release`.
# The release build's cost is `[profile.release]`'s `codegen-units = 1` + thin
# LTO, ~198s of codegen on the single `quire` crate plus the link, a repeat
# deploy measured at 9-11 minutes (docs/BUILD_PERFORMANCE.md). Dev asks for the
# plain tree `just build` / `just run` already keep warm, with no `--config`
# override (that hybrid is what `check`'s comment warns about: it rebuilds all
# 325 dependency crates), so the bump only recompiles `quire` and an ~11s link.
# Since ADR-0168 the dev tree's *dependencies* are built optimised
# (`[profile.dev.package."*"]`), which is what lets this exe animate at all: at
# opt-level 0 the drawer's 220 ms tween painted one frame. The line is in
# Cargo.toml rather than a flag here so the tree keeps one set of fingerprints —
# it costs a rebuild only when that line changes (once: 1040 s, docs/BUILD_PERFORMANCE.md §12).
# The dev exe still carries the bumped FileVersion — build.rs stamps it from
# CARGO_PKG_VERSION, profile-independent.
deploy-workshop:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\deploy-workshop.ps1

# the same deploy, built `--release` — the audited-size exe.
#
# Use this when the file on the shelf *is* the release artifact (a size audit, a
# copy that goes out as the build of record); `deploy-workshop` above is the
# default and costs a dev build. The build is ~2m30s at best and 9-11 min as a
# repeat, and that is one crate's codegen rather than a cold cache: the bump
# invalidates the whole `quire` crate, and `[profile.release]`'s
# `codegen-units = 1` + thin LTO is *both* the audited size choice and the
# fastest setting for this repeat build (measured 2026-09-25 — PERFORMANCE.md
# "Build cost", and ADR-0024's update).
deploy-workshop-release:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\deploy-workshop-release.ps1

# publish a release: bump the patch, build the release exe, and attach two files
# to a GitHub release — Quire-<version>-windows-x64-setup.exe (install\
# build-installer.ps1's Inno Setup program, per-user and no elevation) and
# Quire-<version>-windows-x64.zip (the same single exe `just dist` packages, for
# someone who would rather not install anything).
#
# This is the shell's *delivery*; `deploy-workshop` above is the other door a
# release leaves by, and it advances the version too. Neither replaces the other:
# a release that only went to the workshop is a folder on this machine, and one
# that was only published is a file in a browser's downloads folder.
#
# The three steps that matter are ordered the way the Android shells' publish
# orders them: the bump precedes the build (build.rs stamps the exe's version
# resource, and quire.iss reads AppVersion out of it), the assets are built
# before the push so an Inno Setup that is not installed fails before anything
# leaves the machine, and the bump lands as its own commit before the tag that
# names it. The version comes from Cargo.toml, so nothing here is bumped by hand.
release-publish:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\release-publish.ps1

# installer end-to-end regression (D8/A6): build iss, silent install,
# verify, silent uninstall, residue check (needs Inno Setup)
# --portable end-to-end regression (A1 follow-up): 24 checks over the
# portable layout, log following, migration suppression, --db precedence
verify-portable:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\verify-portable.ps1

verify-install:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\verify-installer.ps1

# remove every build artifact: ./target plus the benchmark and screenshot dirs
clean:
    cargo clean
    Remove-Item -Recurse -Force target-shot, target-skia, target-wgpu -ErrorAction SilentlyContinue

# drop what a build can regenerate *without* giving up incrementality (ADR-0134).
#
# `cargo clean` is all-or-nothing: it throws away the 7.6 GB release tree and
# the next `just deploy-workshop` pays a full rebuild. This recipe instead
# removes only the two things that grow without bound — the incremental caches
# (7.5 GB of them, one directory per unit per session) and the per-unit build
# script output (`target/debug/build`, 6.7 GB, of which 15 near-identical
# 250 MB `build_script_build` copies were the bulk). The compiled artifacts stay,
# so the next build is a link, not a recompile.
sweep:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\sweep.ps1

# what `target` is made of, largest first — for when it is big again
size:
    powershell -NoProfile -ExecutionPolicy Bypass -File install\size.ps1
