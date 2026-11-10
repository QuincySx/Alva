# Design authoring
> Python model of the design system, the generator of alva.smallpen and the Kit review helpers.

## 地位
Offline design tooling for docs/design/alva/alva.smallpen; not Go code, not a runtime dependency. The JSON-format authoring scripts of the earlier packages were deleted on 2026-11-09 (git history, commit 690c4525).
## 逻辑
design_system.py is the Python model (61 tokens, 56 components) the Kit follows. build_next.py resolves every component combination, writes the first combination as the shared element and each distinct difference as an `<sp-variant>` (axes that do not matter are dropped from `when`), restates a copy when its props or overrides change, and writes Token CSS names derived from the Kit names. `shell_deltas()` applies the native-shell structure (Icon.PanelLeft, PageHeader.Navigation and system reserve, Sidebar top row, SettingsNav title row, PanelHeader More); the model was also corrected to the values the saved design and the Kit agree on (control padding 8, Tab height 28, MenuItem padding, TextArea padding, transparent CompanionRow, bordered ResourceRow, Sidebar fill and gap). Number Tokens (line.*, motion.*) travel as string Tokens because SmallPen has no number type; typography composites (`type-*`) and opacity Tokens are derived and not part of the Kit table.
## 约束
build_next.py writes only tokens.css, components/, assets/fonts, index.html (when missing) and token_names.json; it never touches pages/ (pages are edited directly) or requirements/. After every run: `smallpen check docs/design/alva/alva.smallpen --render`. Export PNGs are local review files, never committed. fonttools is an authoring-only dependency of the font/icon tools.
## 业务域清单
| 文件 | 职责 |
|---|---|
| design_system.py / composer_states.py | Python model of the tokens and the 56 components (composer_states.py holds the editor, voice, run-status and menu components) |
| build_next.py | Generator: tokens.css, components/*.html (removes stale ones), fonts, index.html and token_names.json; `--check` compares the package with what it would write |
| token_names.json | Kit token name, CSS variable and type table plus the exact list of derived Tokens (type-*, opacity-*); read by export_tokens.py and by the theme tests |
| export_tokens.py | Offline, read-only export through the smallpen CLI (`token list`, `view --components`) of alva.smallpen to apps/ui/kit (tokens.json, design-components.json); optional output-root argument and ALVA_DESIGN_PACKAGE for tests; unknown, missing or mistyped Tokens fail; never writes the design package |
| export_design.py | `smallpen export --component X --format png` for all or named components into a directory (ALVA_KIT_DESIGN_DIR) |
| icons.py / fonts.py / extend_fonts.py | Offline font and icon asset preparation (pinned Lucide subset, OFL text fonts) with provenance; independent of the package |
