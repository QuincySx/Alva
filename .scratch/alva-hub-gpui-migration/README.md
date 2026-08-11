# ALVA Hub GPUI Migration Tasks

> Current task authority for the native desktop migration. Product decisions
> live in `docs/GPUI-MIGRATION.md` and ADR-0004/0005.

## Accepted Direction

- one desktop product: `alva-hub-gpui`;
- macOS-native V1;
- left projects/sessions, center chat, right collapsible Dock workspace;
- Chat + Alva Inspector before broader Hub visual refinement;
- separate retirement gates for Alva App and Tauri Hub;
- capability preservation, not Tauri page or pixel parity.

## Execution Order

| Order | Ticket | Result |
|---|---|---|
| 0 | [00](./issues/00-freeze-single-gpui-product.md) | Conflicting migration routes are closed. |
| 1 | [01](./issues/01-native-dock-shell.md) | The final three-region native shell exists. |
| 2 | [02](./issues/02-chat-runtime-surface.md) | Hub execution is usable as a live chat. |
| 3 | [03](./issues/03-engine-inspector-contract.md) | Typed durable and live inspection APIs exist. |
| 4 | [04](./issues/04-inspector-tool-workspace.md) | Recursive Alva Inspector works in the right Dock. |
| 5 | [05](./issues/05-retire-alva-desktop.md) | Standalone Alva desktop crates are removed after Gate A. |
| 6 | [06](./issues/06-preserve-hub-capabilities.md) | Existing Hub operations remain usable in GPUI. |
| 7 | [07](./issues/07-retire-tauri-hub.md) | Tauri Hub is removed after Gate B. |

Chat visual styling is intentionally not specified in this task set. It will be
grilled after tickets 01 and 03 establish the actual shell and data boundaries.
