# Hub 引擎 seam 评审 —— 背景与结论

> 日期：2026-08-31
> 范围：`alva-engine-runtime` 契约面 + `alva-hub-core` 路由 + 4 个 adapter + 4 个产品面（desktop / gpui / server / cli）
> 触发：把 DeepSeek Harness「一切皆插件、连内核都可卸载」的理念映射到 ALVA Hub 的「engine 可换」，追问这个设计是否正确。

---

## 核心判断

**方向对，seam 的位置选对了，但缝隙划在了错误的高度。**

准确的表述是三句话，不是一句：

1. **引擎后端在类型层可替换。** `alva-hub-core/Cargo.toml` 只依赖 `alva-engine-runtime`，不依赖任何 adapter；`runtime.rs` 全文引擎名字面量全部在 785 行 `#[cfg(test)]` 之后，生产段零处。内核层可证明地引擎无关。
2. **引擎后端在产品层不可替换。** 第五个引擎能成功注册进 `HubRuntime`，随即死在 `permissions.rs` 的 `(other, _) => Err("unsupported Hub engine")`，接着被 `EngineTarget::for_engine` → `readiness::quick_veto_for_target` → `build_engine` 依次拒绝。**ADR-0007 Finding 2 的「any engine backend can already be registered」在产品路径上不成立**（见工单 15，需一并订正 ADR）。
3. **没有东西「可卸载」，是因为没有东西被「装载」。** 全仓 adapter 零 `Drop` 实现，drop 最后一个 `Arc<dyn EngineRuntime>` 释放零字节、停止零进程。真正缺生命周期的是**一次运行**，不是引擎——加 `unregister_engine` 是十行补丁且修不了任何东西（见工单 13）。

## 统一的失败模式

> **契约里的每一个缺口，都会在产品层重现为一次字符串比较；而每一次字符串比较，都会被按前端复制粘贴一遍。**

这解释了四个看似无关的现象，它们不是四个 bug，是一个架构决定被观察了四次：

- 静默永不触发的 fallback 链（工单 06）
- GPUI 里加不进降级链的 Pi（工单 04）
- hub-server 错误路径上泄漏的引擎句柄（工单 05）
- CLI 里被吞掉的 `--permission allow`（工单 15 的同源背景）

量化：四个产品 crate 生产代码里 engine-id 字面量约 166 处 / 25 个文件；`desktop/permissions.rs` 与 `gpui/permissions.rs` 整文件 `diff` 输出 0 行（412 行逐字节重复）；`readiness.rs` 两份已漂移约 263 行。

## 明确不改的部分

评审确认这些是正确决策，工单不得触碰：

- **4 方法窄 trait**（`execute` / `cancel` / `respond_permission` / `capabilities`）——窄不是问题。
- **`execute` 返回不借 `&self` 的 `'static` Stream**——这是让后面一切都可补救的承重决策，数据面已与实例寿命解耦。5 个生产 adapter 无一作弊。
- **`stream_contract` 作为外置、穷尽 match、可增量喂的检查器**——全设计最硬的一块，且在生产路径上 100% 覆盖（`EngineRuntime::execute` 全仓唯一生产调用点是 `hub-core/runtime.rs:483`，`StreamContract::observe` 挂在 `:517`）。它有骨头，缺的是牙（工单 16）。
- **`EngineInspector` 作为独立可选 trait**——ADR-0004 让 `EngineRuntime` 专注执行的判断是对的。
- **`register_engine` 拒绝静默替换实例的姿态**——但注释里的**理由**已过时，需重写（工单 13）。
- **不采纳 Cordis / 第二个 DI 框架的决定本身**（ADR-0007）。Cordis 解决「对象归谁、何时析构」（进程内对象图）；多引擎聚合是「5 个异构后端、其中 4 个是讲 3 种 JSON 方言的子进程，如何被规约成同一套语义」（跨进程语义规约）。DI 容器对后者帮不上忙。需要的不是容器，是一个 descriptor 结构体和一个 RunHandle。

## 分级

| 级别 | 含义 | 工单 |
|---|---|---|
| **P0** | 今天就在骗用户或花冤枉钱，改动小、当天可做 | 01–06 |
| **P1** | 结构性但局部；多数今天被「每轮重建 HubRuntime」掩盖，常驻 hub-server 落地当天变线上 bug | 07–12 |
| **P2** | 架构收口，修的是「明天不会更糟」 | 13–16 |

## 依赖关系

```
08 (abandon 动词)  ──────► 13 (给「一次运行」建模)
                              │
14 (拍板 switch 权威层) ──────┤   两者都应先于 15 落地，
                              │   否则 descriptor 会被塞进一个待下线的状态机
15 (EngineDescriptor + 协商模块上移)
   ├─ 前置：04/05/06 先止血，避免上移时把已知 bug 一起搬走
   └─ 产出：ADR-0007 Finding 2 订正

16 (conformance harness) —— 独立，可随时开工，越早越能接住 01/02 的回归
```

## 一句话

设计只完成了一半：**「跑一轮」建模得很好（窄 trait + 时序契约，这部分比 DSH 的逐帧 Zod 结构契约更强），没建模「配置」「一次运行」「可选能力」**——这三样缺失在产品层以 166 处字符串比较的形式全额兑现了。不需要推倒，也不需要 Cordis。
