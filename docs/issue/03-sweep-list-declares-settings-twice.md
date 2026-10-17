# 03 · the sweep's scene list declares `settings` twice

**Severity: low** — a bookkeeping error that costs a render and hides one row.

## Symptom

`benchmarks/scripts/sweep.ps1` builds its `$all` list from several blocks and
`"settings"` appears twice:

```
sweep.ps1:43    "dialog", "settings",
sweep.ps1:129   "settings", "dark-settings"
```

The list therefore has 187 rows and 186 distinct names.

## Reproduce

```
node -e '…count "settings" occurrences in $all…'   # see below
```

Parsed out of the script: 187 quoted scene names, 186 unique, `settings` ×2. The
run writes `settings.png` twice — the second render overwrites the first, so the
baseline diff sees one file where the loop ran twice, and a count of "scenes
swept" taken from the list (187) never matches the files on disk (186).

## Evidence

```
.scratch/issue-sweep-20261017/  →  186 PNGs, 0 RENDER-FAIL
```

`ls *.png | wc -l` = 186 while the declared row count is 187.

## Candidate fixes

- Drop the duplicate at line 129 (the `dark-*` block is where a second
  `settings` was clearly meant to be `dark-settings`, which is already there).
- Have `sweep.ps1` fail fast on a duplicate name in `$all` — the sweep is a
  corpus, and a corpus with a repeated entry silently shrinks by one scene.
