# Architecture and reference documents
> Current Go architecture, governance and accepted stack decision.

## 地位
Current implementation authority is in the root Go module and its directory maps. Old Rust product/specification documents are retained only in the Git archive.
## 逻辑
ARCHITECTURE describes current dependencies, owners and physical layout. USAGE contains actual run/build/data instructions; current implementation progress is tracked in root MIGRATION.md. BUS-RULES and REGISTRAR-RULES constrain SDK growth; ADR 0012 records the Go stack and cutover. Dated Go migration/layout records are complete in history; old Rust comparisons remain in the Git archive.
## 约束
Do not treat archived Rust paths or competitor examples as active implementations. Runtime/platform acceptance remains in MIGRATION.md. Preserve accepted boundaries when updating docs.
## 业务域清单
| 路径 | 职责 |
|---|---|
| ARCHITECTURE.md | Current Go architecture, physical layout, owners and consumer-specific storage ports |
| USAGE.md | Root-run commands、live/demo/HTTP、SDK两类WASM与独立JS/WASIp1作者插件、storage conformance/durable APIs和数据生命周期 |
| history/ | Complete dated Go checkpoints and initial repository layout review |
| BUS-RULES.md | Typed capabilities and explicit bounded notifications |
| REGISTRAR-RULES.md | Assembly contribution boundaries |
| adr/ | Accepted Go stack decision and historical archive pointer |
