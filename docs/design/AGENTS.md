# Product design artifacts
> Versioned design packages and their implementation contracts.

## 地位
Product UI design lives outside SDK and runtime code. These artifacts describe target behavior, not platform acceptance.
## 逻辑
Each project owns one current DESIGN.md, editable design packages, CLI authoring inputs and revision-bound review evidence.
## 约束
Respect the chosen authoring tool. Do not hand-edit canonical SmallPen package files or add repository maps inside them. Read App edits before regenerating. Prototype navigation is not execution or permission authority.
## 业务域清单
| 路径 | 职责 |
|---|---|
| alva/ | Preserved wireframes plus formal Tokens, shared UI typography, pinned Lucide icons, editor menus/voice/lifecycle states and product/settings/model/companion pages; SmallPen Web review, CLI pitfalls and Go/HTML ownership plan |
