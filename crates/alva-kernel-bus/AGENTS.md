# alva-kernel-bus
> Cross-layer coordination bus — typed capabilities, pub/sub events, observable state cells

## Role
`alva-kernel-bus` provides three mechanisms for inter-crate communication without
direct dependencies:
- **Caps**: typed capability registry (service locator pattern via `TypeId`)
- **EventBus**: typed pub/sub via broadcast channels
- **StateCell**: observable shared state with change notifications

## Architecture
- **Bus** (`bus.rs`) — top-level owner, not Clone. Creates `BusWriter` (init-phase)
  and `BusHandle` (runtime read-only) from shared Caps + EventBus.
- **BusWriter** (`writer.rs`) — init-phase handle with `provide()` for capability
  registration. Distributed during `BaseAgent::build()`.
- **BusHandle** (`handle.rs`) — runtime read-only handle. No `provide()` method
  (compile-time guarantee). Distributed to middleware, tools, and context layers.
- **BusPlugin** (`plugin.rs`) — two-phase plugin lifecycle: `register()` with
  controlled `PluginRegistrar`, then `start()` with read-only `BusHandle`.
- **Caps** (`caps.rs`) — `Arc<T>` registry keyed by `TypeId`, thread-safe via
  `parking_lot::RwLock`.
- **EventBus** (`event.rs`) — typed broadcast channels, any `BusEvent` type.
- **StateCell** (`cell.rs`) — observable shared state with watch-style notifications.

## Constraints
- No dependency on agent-core, agent-runtime, or any domain crate — leaf dependency
- Compile-time separation: BusWriter (write) vs BusHandle (read-only)
- PluginRegistrar is write-only during register phase (no get/require)
- See `docs/BUS-RULES.md` for anti-degradation rules

## Module Map
| File | Public API | Role |
|------|-----------|------|
| `src/lib.rs` | re-exports | Crate root |
| `src/bus.rs` | `Bus` | Top-level bus owner, creates writer + handle |
| `src/writer.rs` | `BusWriter` | Init-phase handle with provide/get/emit/subscribe |
| `src/handle.rs` | `BusHandle` | Runtime read-only handle with get/require/emit/subscribe |
| `src/plugin.rs` | `BusPlugin`, `PluginRegistrar` | Plugin trait + controlled registrar |
| `src/caps.rs` | `Caps` | Typed capability registry |
| `src/event.rs` | `BusEvent`, `EventBus` | Typed pub/sub broadcast system |
| `src/cell.rs` | `StateCell` | Observable shared state |
