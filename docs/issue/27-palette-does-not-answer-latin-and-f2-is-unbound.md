# 27 · 命令面板认不出拉丁输入，还挂着一个没有绑定的 F2

**Severity: medium** — two thirds of the palette's advertised shortcuts and every
one of its 16 action rows are unreachable by typing, and the failure mode is a
dialog that silently shrinks to a bar.

## Evidence

`node palettesearch.js <state.rs> <AppWindow.slint>`
(`ftest/palettesearch.js`; the rows and the hints are parsed out of
`src/app/state.rs`'s `mock_commands()` and the keys out of
`ui/AppWindow.slint`'s `@keys()` blocks, so nothing in the table is typed from
memory):

```
rows parsed: 16
"new" -> NOTHING      "page" -> NOTHING      "search" -> NOTHING
"theme" -> NOTHING    "dark" -> NOTHING      "setting" -> NOTHING
"notes" -> NOTHING    "tasks" -> NOTHING     "rename" -> NOTHING
"copy" -> NOTHING     "delete" -> NOTHING    "export" -> NOTHING
"import" -> NOTHING   "back" -> NOTHING      "forward" -> NOTHING
"sidebar" -> NOTHING  "edit" -> NOTHING      "navigate" -> NOTHING
"organizer" -> NOTHING "note" -> NOTHING     "todo" -> NOTHING
"markdown" -> EXPORT_PAGE, IMPORT_MD, COPY_MD
"md" -> EXPORT_PAGE, IMPORT_MD, COPY_MD
"html" -> OPEN_HTML
```

22 of the 25 queries return nothing. `markdown`/`md`/`html` do match — that is
the positive control: the matcher is not dead and the harness is not blind, the
16 names are simply written in one script. `fuzzy_subsequence` compares
`char`s, and an ASCII query char can never equal a CJK target char, so *no*
Latin query can match a pure-Chinese name. The three that answer are the three
rows whose Chinese name has an English loanword inside it
(`导出页面为 Markdown…`).

The shortcut half:

```
hint -> is that key declared?
  CMD_NEW_PAGE       "Ctrl+N" -> declared
  CMD_SEARCH         "Ctrl+P" -> declared
  CMD_TOGGLE_SIDEBAR "Ctrl+\" -> declared
  CMD_TOGGLE_THEME   "Ctrl+Shift+L" -> declared
  CMD_RENAME_PAGE    "F2" -> NOT DECLARED anywhere in ui/
  CMD_NAV_BACK       "Alt+Left" -> declared
  CMD_NAV_FORWARD    "Alt+Right" -> declared
```

against the 17 bindings the window actually declares:

```
Control+K  Control+P  Control+N  Control+BackSlash  Control+Z  Control+Shift+Z
Control+Y  Control+Shift+L  Control+F  Control+S  Control+Equals  Control+Plus
Control+HyphenMinus  Control+Digit0  Alt+LeftArrow  Alt+RightArrow  Escape
```

`grep -rn "F2" ui/ src/` returns exactly one hit, and it is the hint itself:
`src/app/state.rs:15101`, `cmd(CMD_RENAME_PAGE, "重命名页面", "F2", "页面",
"pencil");`. So the row tells the user to press a key that no `KeyBinding`
claims. Note that `重命名页面` is a *page* command whose only other door is the
sidebar's own rename, which `06` already shows the harness cannot reach.

What the empty result looks like on screen is geometry, not taste:
`ui/components/CommandPalette.slint:127`

```slint
height: Math.min(UIState.commands.length, 8) * 36px + 40px;
```

— an empty model makes the result area 40 px, i.e. the panel collapses to the
input row, and there is no "no matches" branch anywhere in the file (`grep -n
"length === 0\|没有\|无结果\|empty"` over it: no hits). Type one letter the 16
names do not answer and the dialog appears to have closed itself.

**Labelled as not photographed.** The pass has no pixel evidence for the
collapse: `src/bin/quire_shot.rs` can open the palette but has no way to seed
`palette-query`, so no arm exists that renders a filtered list. The claim is the
formula plus the absence of an empty-state branch, and the smallest thing that
would make it a picture is a `--palette-query <text>` arm in the shot binary.

## Cause

`src/app/state.rs:4842-4853` — the filter consults one field:

```rust
let filtered = if query.is_empty() {
    self.all_commands.clone()
} else {
    self.all_commands.iter().filter(|c| fuzzy_subsequence(query, &c.name)).cloned().collect()
};
```

`CommandRow` is a Slint struct — `ui/Types.slint:366-372`, fields `id`, `name`,
`hint`, `section`, `icon`. `section` is the field the filter ignores, and it holds
`"编辑"`, `"导航"`, `"界面"`, `"页面"`, `"跳转到页面"` — so the row does carry
text beyond its name, but every bit of it is Chinese, which is why falling back to
`section` (below) buys almost nothing.

`state.rs:15135` builds the page rows from the workspace titles:

```rust
cmd(CMD_PAGE_BASE + id, title, "", "跳转到页面", "page");
```

so a library whose pages are named in English *is* searchable, and the gap is
specifically the 16 built-in actions. A user who names pages in Chinese and
types in English gets an empty panel; the same user typing `md` gets three rows.

The two clean halves, recorded so nobody re-checks them:

- `controller.rs:2886-2890` resolves keyboard Enter with
  `s.commands.iter().nth(focus)`, and `commands` **is** the filtered `VecModel`
  (`set_query` calls `self.commands.set_vec(filtered)`), not the full list — so
  the highlighted row is the row that runs. This is the thing that usually breaks
  in a palette and it does not break here.
- `palette_action` (`state.rs:14966-14984`) has an explicit arm per command and
  ends in `None`, and the dispatch has no wildcard — the guard that `aaa3763`
  introduced after a missing import turned a pattern into a catch-all binding.

## Candidate fixes

- Give `CommandRow` a `keywords: string` (one line in `ui/Types.slint:366-372`) and
  filter on `name + keywords + section`. Seed it per row: `new page|create|note`, `theme|dark|light`,
  `settings|preferences|config`, `rename|title`, `export|markdown|md`, and the
  pinyin initials the same user would also type (`xjym` for 新建页面). The data
  is already 16 rows in one function, so this is 16 string literals, not a
  refactor.
- Or, cheaper and uglier: let the filter fall back to `section` and `hint`. That
  gets `Ctrl+N` typed as `ctrl` working and nothing else; it is worth doing only
  as a stopgap.
- Empty state: when `UIState.commands.length == 0`, render one row that says
  没有匹配的命令 — 试试中文 or 关闭 — and hold the panel's height at one row so it
  does not look dismissed. This is also the honest place to say the panel matches
  names, not meanings.
- `F2`: bind it (`ui/AppWindow.slint` has 17 `KeyBinding` blocks, adding an
  eighteenth beside `Control+P` is the same shape) or delete the hint. A hint that
  is not bound is worse than no hint, because the user will press it, see nothing,
  and conclude the rename is broken rather than that the label is wrong.
- Add the missing measurement: a `--palette-query <text>` arm in `quire_shot.rs`
  and one swept scene per interesting query (empty, matched, unmatched). Then the
  collapse is a diff against a control instead of a formula, and the 09/10 family
  of "the dialog does not answer to keys" has a way to be seen.

## How the harness got it wrong first

`palettesearch.js` takes both file paths as arguments, and the first two runs
passed them into the wrong slots (`argv[3]` was serving double duty as the query
list and the `.slint` path). It failed loudly — `ENOENT` on
`.scratch\src\app\state.rs` — which is the right behaviour, but only because the
script prints `rows parsed: 16` and `KeyBinding blocks: 17` before anything else.
A coverage script that printed counts *after* the table would have quietly turned
a 0-row parse into "every hint is undeclared" and produced a record about
shortcuts that do exist. Both counts are now the first lines out, and `16` is the
number `mock_commands()` really has.
