# 29 · 130 inbound firewall rules on this machine are quire's, and each one opens every port for an exe that may no longer exist

**Severity: medium** — not a bug in the code but a cost the code produces, and the
cost is measured here rather than asserted. Windows asks "allow quire access?"
because something binds `0.0.0.0` at every launch, this app's own ADR names that
prompt as a cost worth avoiding, and the residue of answering it is 130 rules that
let one exe accept inbound traffic on **any** port from **any** remote address on
the **Public** profile. Nothing ever reaps them: 37 of the 65 exe paths they name
are already deleted.

## Evidence

Read-only enumeration (`fwp.ps1`, `fwp2.ps1` — `Get-NetFirewallRule` paired with
the application, port and address filters), predicate:
`DisplayName -like '*uire*' -or Name -like '*quire*'`.

```
matched=130
profiles : Public=130
actions  : Allow=126  Block=4
enabled  : True=130
dirs     : Inbound=130
lports   : Any=130
remoteips: Any=130
ruleidkind: QueryUser=130
distinct programs = 65
programs under C:\workshop = 17
```

Every one of the 130 has a rule id of the form `TCP Query User{GUID}` /
`UDP Query User{GUID}` — that is Windows' own naming for a rule created by the
interactive "Windows Defender Firewall has blocked some features of this app"
alert, i.e. a human answer. Two display names recur: `Quire - a local, fast editor
for your own notes` (the exe's own `FileDescription`) and `quire-6f8786ac0b62193c`
(a cargo-built test exe whose binary name carries a hash). They cover paths in dev
trees, in `%LOCALAPPDATA%\Temp\quire-gpui-portable-<guid>\`, in `target\debug\deps\`,
and in the release archive folders `C:\workshop\quire-desktop-0.1.<n>\quire.exe`.

**The denominator, so the size is not just "a lot".**

```
machine-wide QueryUser rules: 250     inbound: 250     allow: 246     distinct programs: 125
```

One app accounts for **130 of 250 user-answered inbound rules (52 %) and 65 of
125 distinct programs (52 %)** on this machine.

**Live against dead** (`fwp2.ps1`, `Test-Path` on each distinct program):

```
rules=130  distinct-programs=65  live=28  dead=37
--- dead paths, by shape ---
installed release = 16      (C:\workshop\quire-* — the per-version archives)
cargo test harness exe = 14 (target\debug\deps\quire-<hash>.exe)
target build = 6
other = 1
--- live paths still allowed inbound on any port ---
dev tree = 23
AppData (installed/share dir) = 4
installed release = 1
```

The `C:\workshop\quire-desktop\quire.exe` rule pair is one of the dead ones right
now (`Test-Path` → False, two Allow rules on Public): the install folder the app is
run from was not present at measurement time, yet the permission for that exact
path is still in the policy store, and a future build written to that path is
allowed inbound the moment it exists.

**The four Block rules** are the same alert answered the other way, and one of
them is this pass's own test target — the sandbox exe has *both* an Allow pair and
a Block pair (Windows gives an explicit Block precedence):

```
Public | TCP Query User{3…} | …\AppData\Local\Temp\quire-gpui-portable-6f3f…\package\quire-gpui.exe
Public | UDP Query User{8…} | (same)
Public | TCP Query User{F…} | …\sandbox-20261017\quire-desktop\target\debug\quire-slint.exe
Public | UDP Query User{A…} | (same)
```

That is why record 28's share test could only be a self-connect: the sandbox exe
is blocked inbound on Public, and a connection from the same host to its own WLAN
address does not consult that rule. **Nothing in this record is a remote
handshake**; it is the policy store, read back.

**The network the machine is on:** `Get-NetConnectionProfile` →
`WLAN / Public`; all three profiles `Enabled=True`, `DefaultInboundAction=NotConfigured`.
So these Allow rules are live on the profile this machine is actually using, not on
a dormant one.

## Cause

- **Something listens on every launch, whoever the user is.** `start_sync` binds
  `("0.0.0.0", SYNC_PORT)` (`controller.rs:267`) before the engine starts, and the
  engine binds it for real; there is no setting in front of it (this is record 21).
  The MCP server is loopback (`mcp.rs:181` binds `127.0.0.1`) and therefore costs
  no prompt; the share is off unless asked (`state.rs:2114-2116`) and costs no
  prompt until turned on. 5878 is the always-on one, and it is `0.0.0.0`.
- **A new exe path every build, and a new one every release.** `docs/DECISIONS.md`
  spells the rule out as a reason for choosing a named pipe over a TCP quit
  channel (ADR-0105, line 2534): *"every new exe that listens is a firewall
  prompt, and this app gets a new exe on every deploy"*. ADR-0106 (line 2455) makes
  the delivery 「a GitHub release of the installer and the portable zip」, and a
  portable zip lands in a new directory per version. Between those two decisions,
  the alert is guaranteed to come back, and the answer the human gave to the last
  one does not carry over — the rule is a path.
- **Nothing reaps.** Three greps, none of which touches `docs/issue` (this record's
  own prose would otherwise match):

  ```
  grep -rni "firewall|NetFirewall|netsh" install/ justfile benchmarks/  → 0 hits
  grep -rn  "netsh|NetFirewall" src/ ui/                                → 0 hits
  grep -rni "firewall" docs/ --exclude-dir=issue                        → 1 hit, DECISIONS.md:2534
  ```

  No installer rule, no uninstall step and no `just` entry touches the policy
  store, and nothing in the app names it — so rules for deleted paths (37 today)
  stay forever. The one sentence in the repository that mentions it is the ADR
  explaining why a *different* channel was chosen to avoid it.
- **The answer Windows records is wider than the thing the user agreed to.** The
  alert says nothing about ports; the rule it writes is
  `localport=Any remoteip=Any profile=<the profile active at that moment>`. So a
  person who clicked Allow once so their tablet could sync has, for that exe path,
  permitted inbound on all TCP *and* all UDP ports.

## Why this is worth a record even though the user clicked

Because the click was solicited by an always-on listener the user did not turn on,
and the consequence is exactly the thing record 28 measures: the share, when a
person does enable it, is served by an exe whose any-port inbound permission was
granted for a different reason, on a Public profile, with no credential on the
socket. The exposure in 28 is conditional on this record; this record is the
condition, and it is 130 lines deep.

## Candidate fixes

- **Stop binding `0.0.0.0` for a service that is off by default.** 5878 is the only
  always-on non-loopback listener; moving it behind the same opt-in row the share
  uses (`sync.enable`, default off, or default on-but-loopback) removes the prompt
  for every future build. This is record 21's fix, and it is also the fix for the
  residue.
- **Bind an interface, not the wildcard.** The app already knows which address a
  peer would dial — `sync_self_address` (`state.rs:15619-15624`) picks the routable
  interface by binding `0.0.0.0:0` and *connecting* to RFC 5737 space. Binding the
  sync listener to that same IP is strictly narrower than `0.0.0.0`.
- **Ship one rule instead of one per answer.** If the installer is to open the
  port, let it add a rule scoped to `proto=TCP localport=5878` (and `5877` only
  while sharing) and let the app bind loopback-plus-that. One documented rule with
  a name the app can find (`Quire sync`) beats 130 `Query User` rules with a name
  derived from `FileDescription`.
- **Add a doctor, and a reap.** `just lan-doctor` printing (a) the listeners and
  their owning pid, and (b) the rules whose `Program` no longer `Test-Path`s, with a
  `--prune` that deletes exactly those. The predicate is already written in
  `fwp2.ps1`; this is a product-side copy of it, not new discovery.
- **Say the sentence in the dialog.** The share's row reads
  「在局域网内只读共享（端口 5877，重启后生效）」 (`SettingsDialog.slint:522`). 只读 is
  true and this pass verified it (405 on POST/DELETE/HEAD). 任何人可以读 is also
  true and is not said. Whatever else changes, that label should not be the only
  place the risk is described — see 28's first candidate.

## Not measured (said plainly)

- **No remote host.** No second machine was used; nothing here proves a peer on
  the segment can connect, only that a rule permitting it exists (for the exes with
  Allow) and that the socket accepts on the WLAN interface (28).
- **Rule creation times** were not compared against exe mtimes, so this record does
  not claim a per-version cadence; the `QueryUser` id shape plus 65 distinct paths is
  the evidence that many separate alerts were answered.
- **Whether Windows would prompt again** for the current build: the sandbox exe
  already has rules (both kinds), so no alert appeared during this pass, which is
  itself the "silently inherits the old answer" half of the finding.
