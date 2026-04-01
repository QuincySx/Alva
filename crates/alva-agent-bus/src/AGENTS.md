# alva-agent-bus/src
> Source files for the cross-layer coordination bus

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| Crate Root | `lib.rs` | Module declarations and public re-exports |
| Bus Owner | `bus.rs` | Top-level Bus struct — creates BusWriter and BusHandle from shared Caps + EventBus |
| Writer Handle | `writer.rs` | BusWriter — init-phase handle with provide(), get(), emit(), subscribe(), handle() |
| Read-Only Handle | `handle.rs` | BusHandle — runtime handle with get(), require(), has(), emit(), subscribe() (no provide) |
| Plugin System | `plugin.rs` | BusPlugin trait (register + start lifecycle) and PluginRegistrar (controlled write-only) |
| Capability Registry | `caps.rs` | Caps — Arc<T> registry keyed by TypeId, thread-safe via parking_lot::RwLock |
| Event Bus | `event.rs` | BusEvent trait + EventBus — typed broadcast channels for pub/sub |
| State Cell | `cell.rs` | StateCell — observable shared state with watch-style change notifications |
