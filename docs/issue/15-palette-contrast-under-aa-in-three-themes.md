# 15 · three themes carry text and status colours below WCAG AA

**Severity: low** — a static measurement of the shipped palette table, not a
rendered defect. Reported because the numbers are one subtraction away from the
tokens and the check costs nothing to keep.

## Method

`.scratch/issue-test/contrast.js` parses the `ThemePalette` array in
`ui/Colors.slint` (12 entries, ActivityWatch's `kThemes[]` table) and computes
WCAG 2.1 ratios for the pairs that carry meaning on screen. No app involved; the
input is the same hex the renderer uses.

| theme | 正文/底 | 次要/底 | 次要/卡片 | 主色/底 | 危险/底 |
|-------|--------:|--------:|----------:|--------:|--------:|
| midnight | 13.55 | 6.69 | 6.02 | 5.09 | 4.57 |
| graphite | 14.64 | 5.62 | 5.07 | 4.62 | 5.16 |
| violet | 14.62 | 6.47 | 5.84 | 6.63 | 6.53 |
| emerald | 15.03 | 7.82 | 7.07 | 9.26 | 6.44 |
| amber | 14.39 | 7.50 | 7.02 | 8.28 | 6.43 |
| ocean | 14.76 | 7.81 | 7.19 | 9.81 | 6.58 |
| rose | 14.87 | 7.57 | 7.09 | 6.83 | 5.01 |
| **light** | 13.55 | **4.47** | 4.83 | **4.21** | 4.56 |
| **jade** | 8.79 | **4.10** | 4.64 | 5.38 | **3.74** |
| deepblue | 12.17 | 5.58 | 5.23 | 6.79 | 5.26 |
| twilight | 12.77 | 5.61 | 5.19 | 5.82 | 5.56 |
| **crimson** | 12.59 | 6.01 | 5.65 | 5.42 | **3.19** |

Body text (`fg`) passes AA in all twelve, comfortably. The shortfalls:

- **`light` / `fg-muted` = 4.47** (`#6b7280` on `#f5f6f8`) — one hundredth under
  4.5. `fg-muted` is not decoration: it is the count column, the breadcrumb, the
  `清单` section caption (`OrganizerArea.slint:3029`) and every inactive nav row.
- **`jade` / `fg-muted` = 4.10** (`#8fa8a6` on `#0f4938`) — jade's `bg` is a mid
  green, so the muted token has less room than in the near-black themes. Jade is
  also a *gradient* theme (`bg #0f4938` → `grad2 #0a2442`), so the same token sits
  on two different grounds and only the top one is measured here.
- **`crimson` / `danger` = 3.19** and **`jade` / `danger` = 3.74** — the delete
  colour, at the size a destructive label is drawn, is below 4.5 in two palettes.
- **`accent` in `light` = 4.21** (`#2f6fed` on `#f5f6f8`) — the accent is a
  *fill* far more often than a text colour, so this matters only where accent
  carries a glyph (link text, `Colors.accent-text` buttons are the other side).
- All twelve `border` tokens measure **1.14–1.64** against `bg`. That is normal
  for a hairline and not an AA *text* failure, but WCAG 1.4.11 asks for 3:1 on
  component boundaries whose state must be perceivable — relevant to
  `light`/`jade` where the border is the only thing separating a card from the
  page.

## Candidate fixes

- Nudge three values rather than re-deriving the catalog: `light`'s `fg-muted`
  (`#6b7280` → e.g. `#5f6670`, 4.5+), `jade`'s `fg-muted`, and `crimson`/`jade`
  `danger`. The catalog is a port of ActivityWatch's table "value-for-value"
  (`Colors.slint:4-7`), so any change is a documented deviation from the reference
  and wants a line in the theme ADR saying so.
- Keep the measurement: put `contrast.js` under `benchmarks/scripts/` and print the
  sub-4.5 list from `rc_gates.ps1`, the same way `audit_results.ps1` recomputes
  stored numbers. A palette catalog that is edited by hand will drift back.
- Decide the gradient-theme question explicitly: for `jade`/`deepblue`/`twilight`/
  `crimson`, measure against `grad2` as well and use the *worse* of the two stops as
  the token's floor — currently only the top stop is checked by anything.
