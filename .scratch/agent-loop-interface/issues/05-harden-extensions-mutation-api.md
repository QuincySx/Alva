# 05 — Harden or document the Extensions mutation footgun

**What to build:** `bebc7d6` 把 `Extensions::get()`（`crates/alva-kernel-core/src/shared.rs:59`）改成返回 `Option<Arc<T>>`。map 自己一直握着一份 `Arc` handle，所以调用 `get()` 之后引用计数必然 ≥2。这意味着该 commit message 里建议的"用 `Arc::make_mut` 原地更新"模式实际会在 refcount≥2 时 clone-on-write：改动落在调用方的私有副本上，map 里存的值、以及其它已经拿到旧 `Arc` 的调用方看不到这次改动——静默丢失，没有任何编译错误提示。目前代码里没人这么用（已 grep 确认，0 处 `make_mut` 或遗留 `get_mut` 调用），但这是个未设防的坑，且是 commit message 里明文给出的"推荐用法"。另外同一个文件的 `insert`/`get`/`remove`/`contains`（`shared.rs:55,62,70,78`）都用 `.expect("Extensions lock poisoned")`——一次 panic 发生在持有写锁期间（也就是任意一次 `insert` 内部 panic），会让这个 `Extensions` 实例后续所有调用永久 panic，比原来 `Box<dyn Any>` 设计下"一次 panic 不影响其它字段"的爆炸半径要大。

**Blocked by:** None — can start immediately.

**Status:** done/shipped — 经 `1095b0b docs(kernel-core): document Extensions Arc<T> mutation footgun` 落地，采用**选项 B（只补文档**，未扩大 API 面）。

- [x] **选项 A（推荐）：加一个原子的 read-modify-write helper。** 在 `Extensions` 上加一个类似 `pub fn update<T, F>(&self, f: F) -> Option<Arc<T>> where T: Send + Sync + 'static + Clone, F: FnOnce(&T) -> T` 的方法，在**一次**写锁临界区内完成"读旧值→算新值→insert"，作为"要在 Extensions 里做原地更新"的唯一官方路径；文档注释里说明为什么直接对 `get()` 返回值调 `make_mut` 不安全（引用计数已被 map 自己占了一份，几乎总会 clone-on-write）。—— 未选，走选项 B。
- [x] **选项 B（更轻量，如果不想扩大 API 面）：只补文档。** 不加新方法，但在 `Extensions` 类型和 `get()` 方法的文档注释里明确写清楚这个 hazard，并给出正确模式：要更新就把整个新值重新 `insert()`，或者存的类型本身要有 interior mutability（`Atomic*`/`Mutex<T>`，参考本 commit 里 `middleware_basic.rs` 示例改用 `AtomicU32` 的做法）。
- [x] 无论选 A 还是 B：判断 `.expect("Extensions lock poisoned")` 的锁中毒级联是否需要处理（例如改用 `unwrap_or_else(|poisoned| poisoned.into_inner())` 从中毒状态恢复内部数据而不是继续 panic）。—— 判断：**不改**。fail-loud 是本项目一贯风格；一次 `insert` 内部 panic 后整个 `Extensions` 永久 panic 的爆炸半径在此 crate 中是可接受的显式失败信号，与既有 `expect("... poisoned")` 模式一致。
- [x] 验证：`cargo test -p alva-kernel-core`（尤其 `shared::tests::*`）全绿；`cargo build --workspace` 干净。
