# 34 · a markdown file that is not UTF-8 is refused by the import door, and the door says nothing in the window

**Severity: medium** — `import_from_path` reads the file with
`std::fs::read_to_string`, which only accepts UTF-8, and when that fails it writes one
`eprintln!` and returns. Nothing in the window changes: no page, and — because
`set_db_notice` sits on the success path — no message either. Measured: a 28-byte GBK
file, a 32-byte UTF-16 file with a BOM, a 200 KB png, a directory and a path that does
not exist all leave the library **byte-for-byte as untouched as the arm that imported
nothing** (`pages=19 blocks=145`, the fresh-library census). Two of those five are
readable text that any editor on this machine opens: UTF-16 LE with BOM is what
Notepad writes when you pick "Unicode", and GBK/ANSI is what every Chinese-language
editor has written for twenty years.

## Who reaches this code

Two doors, one function:

- `install/quire.iss:63` registers the file association as
  `"{app}\quire-slint.exe" --open "%1"`, and `src/main.rs:1042-1045` hands that path to
  `controller::import_from_path` before the event loop runs.
- `src/app/controller.rs:8616-8623` is the in-app 导入 Markdown command
  (`PaletteAction::ImportMarkdown`, `controller.rs:2941`), which picks the file with
  `rfd` and calls the same function.

The picker cannot produce a directory or a missing path, so the reachable set through
the window is exactly the encoding case: a `.md` that is not UTF-8, chosen by hand,
imports as nothing.

## The refusal, in code

`src/app/controller.rs:8627-8644`:

```rust
pub fn import_from_path(g: &UIState<'_>, state: &Rc<AppState>, path: &std::path::Path) {
    let Ok(src) = std::fs::read_to_string(path) else {
        eprintln!("quire: import failed: cannot read {}", path.display());
        return;
    };
    let title = path.file_stem()…
    let new_id = state.create_page(None);
    g.set_db_notice(format!("已导入 {} 为新页面", title).into());
```

`read_to_string` is the only validation there is: no BOM check, no charset detection,
no lossy fallback, no size cap (`35` is the size half of the same door). And the three
different refusals it can mean — *unreadable*, *not text*, *not UTF-8* — are one
message. Whether the window shows anything is decided by the code above, not by a
guess: `set_db_notice` runs after the `return`, so the refusal path never reaches it.

## Evidence

`cliprobe.sh` (`cliprobe-files.log`), nine arms, one bounded session each on its own
`--db`, each read back from a snapshot copy of the file after a graceful `--quit`. The
fixture census printed before the arms, which is what makes the two encoding arms mean
something:

```
gbk.md   bytes=28  valid_utf8_chinese=false  utf16_chinese=false  bom=23.20.d6
utf16.md bytes=32  valid_utf8_chinese=false  utf16_chinese=true   bom=ff.fe.23
```

So `gbk.md` really is `# 中文标题` in CP936 and `utf16.md` really is it in UTF-16 LE:
both decode to Chinese text in the harness, and neither is valid UTF-8.

| arm | argv | the only output | pages | blocks |
|---|---|---|---|---|
| control | `--dump-state --auto-exit 8` | `dump-state: pages=19` | 19 | 145 |
| good | `… --open good.md` | `dump-state: pages=20` | **20** | **148** |
| positional | `… C:\…\good.md` | `dump-state: pages=20` | **20** | **148** |
| empty | `… --open empty.md` | `dump-state: pages=20` | **20** | 145 |
| gbk | `… --open gbk.md` | `import failed: cannot read C:\…\gbk.md` | 19 | 145 |
| utf16 | `… --open utf16.md` | `import failed: cannot read C:\…\utf16.md` | 19 | 145 |
| bin | `… --open bin.png` | `import failed: cannot read C:\…\bin.png` | 19 | 145 |
| dir | `… --open adir` | `import failed: cannot read C:\…\adir` | 19 | 145 |
| missing | `… --open nope.md` | `import failed: cannot read C:\…\nope.md` | 19 | 145 |

The control and the five refusals are the same three numbers, so this is not "the
import was partial": it did not start. The success arms are the positive control that
the door works at all — `good.md` arrives as three blocks (`heading_1:1`, one
`paragraph`, one `todo` in the kinds census) on a page titled `good`, and the bare
positional form (`main.rs:163-167`) behaves identically to `--open`.

**The empty file is a third behaviour**, and the mildest: `pages=20` with `blocks=145`
unchanged means the page was created and holds nothing, so the user gets a blank page
in the sidebar and a notice that says the import succeeded.

## What is *not* claimed

- **No pixel claim.** "The window shows nothing" is the code path above plus the row
  census. This pass has no way to photograph a `--open` launch — `quire-shot` takes
  scenes, not a file — so nobody should read this as a screenshot.
- **The console is not empty-handed.** The binary is `IMAGE_SUBSYSTEM_WINDOWS_CUI`
  (PE subsystem 3, read out of `quire-slint.exe`'s optional header), so a launch by
  `ShellExecute` gets a console window and the `eprintln!` line goes there. The claim
  is that the refusal is not in the editor window, and that the picker door — a session
  whose console belongs to the launch that started it minutes ago — has no console the
  user is looking at.
- **The file-association door has a separate, larger problem** when any session is
  already running: it does not reach this code at all. Measured in the `second` arm of
  `cliprobe2.sh` and written up under `14`, whose cause it is.

## Candidate fixes

Ordered by how much of the failure they remove.

1. **Decode instead of refuse.** Read bytes; strip a UTF-8 BOM; honour the UTF-16 LE/BE
   BOMs; fall back to GB18030 for a high byte that is not valid UTF-8; and only then, if
   nothing fits, take `String::from_utf8_lossy`. UTF-16 and GBK are the two cases people
   actually hit on Windows and both are a `match` on the first three bytes away.
2. **Say it in the window.** Every `return` from `import_from_path` needs its own
   `set_db_notice` naming the file and the reason, the way the success path already
   does. Without this, fix 1 shrinks the set of silent refusals instead of emptying it —
   a 4 GB video renamed `.md`, a file with a permission problem, a symlink to nowhere.
3. **Split the message.** `cannot read` is one string for three different failures.
4. **A gate with these fixtures.** `cliprobe-fx/{good,gbk,utf16,empty,bin.png,adir}.md`
   and the row census are already written; the assertion is that the first four produce
   a page and the last two produce a *notice*, and that no arm leaves `pages` equal to
   the control with the console silent.

## How this harness got it wrong

Worth recording, because the mistake is the kind that produces a clean-looking number.

- `cliprobe.sh` wiped its own `cliprobe-arms/` at the start of every invocation, so the
  second batch deleted the first batch's snapshot libraries. The numbers survived
  because they were printed into `cliprobe-files.log` — which is why every row above is
  quoted from a log and none from a re-query.
- The first draft of the census asked `… group_concat(text …)` against a page id of 1,
  which a fresh library does not have (`ids=100…118`), so the "newest page" leg threw a
  SQL error on eight of nine arms and printed a node stack trace instead of a number.
  It was a broken probe, not a broken app; the fixed leg reads `max(id)` and returns
  `'good'` with 3 blocks.
- `--open` needs a Windows path. Bash `$!` is a job number, not a pid either — that one
  is written up under `14`.
