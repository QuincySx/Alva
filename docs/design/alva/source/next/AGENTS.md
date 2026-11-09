# New-format design authoring
> Python model to SmallPen HTML package, plus the Kit review helpers.

## 地位
Offline design tooling for docs/design/alva/alva-next.smallpen; not Go code, not a runtime dependency. Imports ../design_system.py (the component model the Kit follows).
## 逻辑
build_next.py resolves every component combination of the Python model, writes the first combination as the shared element and each distinct difference as an `<sp-variant>` (axes that do not matter are dropped from `when`), restates a copy when its props or overrides change, and writes Token CSS names derived from the Kit names. Deltas of the saved legacy package that design_system.py never received are applied in `shell_deltas()` (Icon.PanelLeft, PageHeader.Navigation and system reserve, Sidebar top row, SettingsNav title row, PanelHeader More); the Python source itself was corrected where the saved package and the Kit agree against it (control padding 8, Tab height 28, MenuItem padding, TextArea padding, transparent CompanionRow, bordered ResourceRow, Sidebar fill and gap). Known remaining differences from the saved package: Composer control height (saved 28, source/design 32), PanelHeader height (saved 36, decided 52), toolbar row height (saved 32 from a stale binding, decided 36). Number Tokens (line.*, motion.*) travel as string Tokens because SmallPen has no number type; typography composites (`type-*`) and opacity Tokens are derived and not part of the Kit table.
## 约束
build_next.py writes only the generated files of alva-next.smallpen (tokens.css, components/, token_names.json) and never pages/; pages_from_legacy.py writes pages/ and docs/design/alva/requirements/. After every run: `smallpen check docs/design/alva/alva-next.smallpen --render`. Never run against the legacy packages. Export PNGs are local review files, never committed.
## 业务域清单
| 文件 | 职责 |
|---|---|
| build_next.py | Generator: tokens.css, components/*.html (removes stale ones), fonts, index.html and token_names.json; `--check` compares the package with what it would write |
| token_names.json | Kit token name, CSS variable and type table plus the exact list of derived Tokens (type-*, opacity-*); read by ../export_tokens.py and by the theme tests |
| pages_from_legacy.py / legacy_flows.json | One-time conversion of the 72 legacy pages (reads the legacy JSON packages and the alpha.21 `flow list` output) into pages/ and requirements/*.md; refuses to rerun without --force because pages are the package's source of truth; delete with the legacy packages |
| compare_legacy.py | One-time check of every converted page against the legacy rendering: visible texts (spec) and the layer list (wireframe) of both sides, optionally the light-desktop PNGs; needs the alpha.21 CLI and the legacy package, remove with them |
| export_design.py | `smallpen export --component X --format png` for all or named components into a directory (ALVA_KIT_DESIGN_DIR) |
