# Design authoring and review
> Public SmallPen CLI operations and reproducible review inputs.

## 地位
Offline design tooling, outside Go application code and runtime dependencies.
## 逻辑
The preserved framework wireframes remain immutable review references. The current authorized phase maintains semantic Tokens, one pinned Lucide icon family, a formal component library and component-composed product/settings pages on a separate canvas. Earlier export helpers remain scoped to the deferred detailed draft. Inspect App changes before any later authoring run.
## 约束
Never write canonical package JSON directly. Batch identity is retry identity, not a way to overwrite App edits. Old committed batch IDs can be superseded; select only affected definitions and use a new, reviewed change. Preserve font provenance. Python authoring dependencies are temporary tooling, not Go dependencies.
Read [the CLI usage and pitfalls guide](../CLI-GUIDE.md) before running authoring scripts. Current formal-page layout is owned by the saved packages and boundary_refresh.py deltas; formal_refresh.py retains the prior composition changes. Earlier product_pages/reading_pilot assemblies must not be run alone over later edits; preserve General action nodes and requirement bindings. Existing generators are authoring inputs, not a blanket recommendation to regenerate all pages; keep content and fill/hug sizing out of unnecessary Cartesian expansion. Read the current saved definitions before recompiling a component; palette/source defaults do not replace later native finishing or instance overrides.
Legacy format: every script below except design_system.py (the model build_next.py reads), export_tokens.py and next/ writes the old JSON packages (old format, to be removed together with alva-foundation.smallpen and alva.smallpen once alva-next.smallpen is confirmed); do not run them against the new package.
## 业务域清单
| 路径 | 职责 |
|---|---|
| native_shell_refresh.py | One-time exact-revision guarded public CLI migration of native-inset Header36/SidebarTopRow, real hidden navigation, existing Settings/Sheet return actions and necessary Expanded/Collapsed/PanelLeft semantics; no direct canonical JSON writes or Go implementation |
| next/ | New-format authoring: build_next.py generates alva-next.smallpen (tokens.css, 56 components) from design_system.py; export_design.py exports per-component PNG sheets for the Kit gallery; token_names.json is the CSS-name/Kit-name table |
| export_tokens.py | Offline, read-only export through the smallpen CLI (`token list`, `view --components`) of alva-next.smallpen to apps/ui/kit (tokens.json, design-components.json); optional output-root argument and ALVA_DESIGN_PACKAGE for tests; unknown, missing or mistyped Tokens fail; never writes the design package |
| author.py | Composition helpers and shared/basic page authoring through named CLI intents |
| workbench.py | Four workspace screens, shared composer and right workspace shell with file/empty/background bodies, closable tabs and panel catalog; reviewed revision required before redraw; setup is one-time |
| model_picker.py | One model-first picker with favorites and Provider filters; left settings panel chooses compatible Agent and model options; direct row selection, four entry/exit/settings links; no state-board copies |
| provider_setup.py | Historical wireframe Provider/model manager, three connection steps and shared model add/edit board; preserved reference, not the current formal provider-flow contract |
| companions.py | Long-lived companion conversation/profile with own and referenced Skills, group conversation and create-group form; product-owned collaboration/visibility policy, no kernel changes |
| design_system.py | Shared semantic Tokens/block recipes and 56 components; independent ProviderForm source/ProviderModels state, compact SettingRow Select/Switch/Action and shared Switch focus, width-preserving Button loading with centered Text overlay and retained icon slots, one popover recipe, formal UI font role, bounded states, six spacing steps, five text sizes, explicit alignment and typed nested overrides |
| composer_states.py | Shared editor lifecycle, recording/transcription, run status, permission/menu states and local8/32/circular-send geometry; no Cartesian configuration expansion or global Button changes |
| composer_surfaces.py | Pure cleanup of the former per-page overlay roots and their own links; keep shared Foundation surfaces and native behavior bindings rather than regenerating hover examples |
| smallpen_web_layout.mjs | Opt-in process-only repair for exact alpha.18 modules: native ComposerMenu/MenuItem component catalogs and deferred system samples, visibility-aware menu sample bounds, secondary roots, SettingsNav/connection-wizard consumers and transparent wrappers; three hash guards, bounded cache, no installed/canonical writes, remove after upstream repair |
| smallpen_layout_worker.mjs | Disposable native measurement thread for the preview repair; explicit request/errors, no writes/server/UI; parent admits one native request with at most16 waiting, sends assets rather than the component catalog, and retires the WASM/font heap after one idle second |
| controls_refresh.py | Public CLI compilation in an isolated package; guarded control replacement and existing instance migration preserving node IDs, text and navigation |
| quiet_refresh.py | Isolated public CLI compilation of the restrained visual system; guarded Token pruning and formal-page refresh preserving wireframes, node identities and prototype navigation |
| rules_refresh.py | Shared Token and block-rule consolidation; isolated compilation, duplicate theme-scope migration, guarded writes, preserved variant/node identities and historical pages |
| product_pages.py | Formal workbench, compact shared preference rows, source-first preset/custom Provider forms with optional inline models, companion/group and complete settings assemblies; explicit top returns, restrained titles, shared ModelForm add/edit modes, public typed overrides and idempotent navigation; an optional revision-matched public Web projection compiles links into one guarded atomic CLI batch |
| formal_refresh.py | Current formal-page composition deltas: compact shared settings shell, connected catalog toolbar, explicit General Save/Reload, bounded chat transcripts and truthful optional metrics/status; isolated native candidate, preserved IDs/links and revision-bound apply; no page regeneration |
| boundary_refresh.py | Propagate the user-approved visual baseline into existing21–43 via isolated native candidates and guarded CLI writes; consolidate surfaces/radii, rebase stale shell paint overrides, group replies without losing original action/requirement IDs; no new Tokens/components/variants or production implementation |
| input_refresh.py | Approved shared Input/TextArea/Field/Composer appearance delta and isolated review preparation: subtle ordinary fields, regular labels and shadow-free writing surface; preserve geometry, focus/error and active setup controls; formal guarded receipts in input-family exports, no new Tokens/variants |
| reading_pilot.py | Reading/wayfinding composition on 21/22; theme-bound identity, readable prose and transcript viewport; isolated guarded CLI refresh retaining existing nodes/navigation without expanding Tokens or variants |
| density_refresh.py | Public typed compact-density deltas for existing Sidebar/Composer and 21/22, revision/dry-run guards and unchanged behavior identities; no page regeneration or new Tokens/variants |
| icons.py | Reproducible 68-symbol subset of pinned official Lucide outlines; stable Alva mappings, original canvas/advances and xMin side bearings, bounded curve conversion and font provenance; no custom icon drawings or runtime dependency |
| extend_fonts.py | Append required glyphs while preserving outlines/advances; derive the formal UI family from source typo metrics, preserve historical fonts, and verify TTF/WOFF equivalence |
| composer_menu.py | One shared add-menu wireframe above the composer: files/media, goal, plan, collaboration and directory; representative open/close links; draft/mode logic is design-only |
| right_workspace.py | One local component board for right-workspace empty state, opened tabs/background empty state and add-tab menu; representative navigation; no task execution ownership |
| states.py | Selection, feedback, focused/docked/running/stopped/error states and remaining settings |
| flows.py | Same-canvas navigation, questions/Other/save/resume and approval outcome demonstrations |
| fonts.py | Offline OFL font instantiation, subset, rename and hash manifest |
| review.py | Deferred detailed draft's native renderer exports and explicit combination review; does not cover current framework boards |
| preview.py | Deferred detailed draft's offline viewer and graph checks; current review happens in SmallPen |
| browser_review.mjs | Disposable Chrome inspection of actual local viewer hotspots, focus/Escape, Other/save/resume/stop, nonmodal Inspector and local-only asset loading |
| app_review.mjs | Installed macOS SmallPen App inspection in a disposable profile; selects a frame without editing the design, captures rendered canvas and checks the saved revision |
| intents/ | Exact typed CLI write intents; descriptive names identify the owning component or page |
| authoring-log.jsonl | CLI write receipts, including refused attempts; historical revisions are not final acceptance |
