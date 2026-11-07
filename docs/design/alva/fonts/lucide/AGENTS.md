# Lucide icon source subset
> One pinned family for all Alva interface symbols.

## 地位
Official lucide-static 1.61.0 SVGs and outlined font paths, under the complete upstream ISC/MIT license. These are design resources, not a JavaScript runtime dependency.
## 逻辑
`glyphs.json` binds stable Alva names/codepoints to official icons and carries the verified release tarball integrity. `source/icons.py` converts the official outlined paths to the SmallPen font without per-icon repositioning. Go/HTML may use the original SVGs directly.
## 约束
Do not hand-draw replacements or mix icon families. Keep the original 24-unit canvas, round 2-unit strokes and semantic names. Source/version upgrades require a reviewed mapping, hash verification and 16/20-size visual checks. Keep LICENSE with SVGs and derived font distributions; retain Feather attribution.
## 业务域清单
| 路径 | 职责 |
|---|---|
| *.svg | 68 selected, unmodified official Lucide vector assets |
| glyphs.json | Pinned tarball/integrity, original font SVG hash, stable mappings and official outlined glyph paths |
| LICENSE | Complete upstream Lucide ISC and Feather MIT copyright/permission notices |
