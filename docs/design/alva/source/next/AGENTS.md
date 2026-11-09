# New-format design authoring
> Python model to SmallPen HTML package, plus the Kit review helpers.

## 地位
Offline design tooling for docs/design/alva/alva-next.smallpen; not Go code, not a runtime dependency. Imports ../design_system.py (the component model the Kit follows).
## 逻辑
build_next.py resolves every component combination of the Python model, writes the first combination as the shared element and each distinct difference as an `<sp-variant>` (axes that do not matter are dropped from `when`), restates a copy when its props or overrides change, and writes Token CSS names derived from the Kit names. Two deltas of the saved legacy package that design_system.py never received are applied in `shell_deltas()` (Icon.PanelLeft, PageHeader.Navigation). Number Tokens (line.*, motion.*) travel as string Tokens because SmallPen has no number type; typography composites (`type-*`) and opacity Tokens are derived and not part of the Kit table.
## 约束
Writes only alva-next.smallpen and token_names.json. After every run: `smallpen check docs/design/alva/alva-next.smallpen --render`. Never run against the legacy packages. Export PNGs are local review files, never committed.
## 业务域清单
| 文件 | 职责 |
|---|---|
| build_next.py | Generator: tokens.css, components/*.html (removes stale ones), fonts, index.html and token_names.json; `--check` compares the package with what it would write |
| token_names.json | Kit token name, CSS variable and type table plus the exact list of derived Tokens (type-*, opacity-*); read by ../export_tokens.py and by the theme tests |
| export_design.py | `smallpen export --component X --format png` for all or named components into a directory (ALVA_KIT_DESIGN_DIR) |
