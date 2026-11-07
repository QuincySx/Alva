# Prototype font resources
> Redistributable text/monospace fonts and one coherent Lucide icon family.

## 地位
Design assets under SIL OFL 1.1 (text fonts) and ISC/MIT (Lucide icons); not an application font dependency decision.
## 逻辑
source/fonts.py instantiates and subsets Noto Sans SC, renamed Alva Prototype Sans SC for preserved history. Formal ui.font.family uses Alva UI Sans SC, the same outlines/advances with the source's existing typographic line metrics; the historical family stays intact. JetBrains Mono supplies the code/log role. source/icons.py adapts pinned official Lucide outlines; original SVGs remain available for Go/HTML. Each source retains license and hash provenance.
## 约束
Keep every applicable OFL/ISC/MIT notice with redistributed assets. Do not substitute proprietary system fonts. Refresh Chinese glyph coverage before CLI import, preserving earlier text outlines/advances. All 68 interface symbols now come from Lucide 1.61.0; do not mix icon families or redraw approximations. Preserve stable names/codepoints, the upstream 24-unit canvas and 1000-unit advance/ascent, zero descent, and each outline's xMin side bearing. Do not center individual ink bounds or use per-icon offsets. The v2.0 Lucide subset supersedes the earlier original artwork while retaining the alignment contract; historical reviews retain their original revisions. Code/log font fallback, large text and platform rendering require separate runtime acceptance.
## 业务域清单
| 路径 | 职责 |
|---|---|
| AlvaPrototypeSansSC-Regular.ttf | Weight 400 subset |
| AlvaPrototypeSansSC-SemiBold.ttf | Weight 600 subset |
| AlvaPrototypeSansSC-Regular.woff | Expanded shared 400 subset with preserved earlier outlines/metrics and identical TTF/WOFF tables |
| AlvaPrototypeSansSC-SemiBold.woff | Same weight 600 outlines/metrics in the imported WOFF format |
| OFL.txt | Original copyright notice and complete SIL OFL 1.1 license |
| NOTICE.json | Source, derivation and SHA-256 provenance |
| AlvaUISansSC-Regular.ttf | Formal 400 UI role, source typo metrics and preserved glyph outlines/advances |
| AlvaUISansSC-Regular.woff | Same 400 UI font tables for Web/native |
| AlvaUISansSC-SemiBold.ttf | Formal 600 UI role with the same metric contract |
| AlvaUISansSC-SemiBold.woff | Same 600 UI font tables for Web/native |
| UI-NOTICE.json | UI derivation, source/metric rationale, OFL attribution and hashes |
| JetBrainsMono-Regular.ttf | Pinned upstream v2.304 regular monospace font for code/logs |
| JetBrainsMono-Regular.woff | Format conversion preserving the upstream outlines and metrics |
| JetBrainsMono-OFL.txt | Complete upstream SIL OFL 1.1 notice |
| JetBrainsMono-NOTICE.json | Pinned commit, source URL, conversion and hashes |
| AlvaInterfaceIcons-Regular.ttf | Lucide-only 68-symbol font subset, stable names/codepoints and original canvas |
| AlvaInterfaceIcons-Regular.woff | Identical subset outlines/metrics for SmallPen Web and native rendering |
| icons.json | Stable Alva-to-Lucide mapping, pinned release, derivation, licensing and hashes |
| lucide/ | Official selected SVGs, outlined glyph source data and complete ISC/MIT license; no runtime package install required |
