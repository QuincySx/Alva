# 14 拍板 switch / handoff 的权威层

- 级别：P2
- 影响：「换引擎继续这轮对话」这一个用户动作在仓里有两套互不相干的实现，旗舰产品走的是另一套；内核那套的 bug 只有 CLI 用户会撞上，长期无人体检，而它已经和 ADR-0002 写下的承诺相矛盾。
- 状态：TODO

## 根因

**同一个产品概念有两套互不相干的状态机，旗舰产品用的是另一套。**

本工单要交付的是一次**拍板**（哪一层是 switch 的权威实现）+ 一次落地。下文的「方向」是待选项，不是既定结论。

## 证据

### A. hub-core 的 handoff 状态机，生产消费者只有 hub-cli

- `rg -n 'switch_engine' crates/ --glob '*.rs'` 的生产命中（排除 `alva-hub-core/src/runtime.rs` 自身定义与其 `:785` 之后的测试段）只有两处，都在 CLI：`crates/alva-hub-cli/src/main.rs:182` 与 `:531`。
- `rg -n 'with_handoff_context' crates/ --glob '*.rs'` 的生产命中只有一处：`crates/alva-hub-cli/src/main.rs:648`。
- `pending_handoff` / `previous_engine_id` / `previous_engine_session_id` / `restore_failed_handoff` 这四个标识符，在 hub-core 之外的生产代码里**只有 hub-cli 读**（`crates/alva-hub-cli/src/main.rs:275,350,466-469,534,601-604,902-906`，配套字段定义在 `crates/alva-hub-cli/src/session_store.rs:35-41`）。desktop / gpui / server 三个产品对这四个字段零引用。

### B. 旗舰产品的 switch 完全不经过 HubRuntime

- `crates/alva-hub-server/Cargo.toml` 的 `[dependencies]` 里**没有 `alva-hub-core`** —— 它只依赖 `alva-hub-desktop` + `alva-engine-runtime`。`rg -n 'alva_hub_core' crates/alva-hub-server/` 零命中。hub-server 连 `HubRuntime` 这个类型都看不见。
- `crates/alva-hub-server/src/main.rs:1046` 注册路由 `POST /api/sessions/:id/switch` → handler 在 `:381`，其落点是 `crates/alva-hub-server/src/main.rs:414-417` 的 `state.store.request_engine_switch(&id, &canonical, &body.reason)`，实现在 `crates/alva-hub-desktop/src/store/branch.rs:208`。
- desktop 与 gpui 的 `HubRuntime` 全是函数局部变量、每轮 `new`：`crates/alva-hub-desktop/src/run.rs:137,526`、`crates/alva-hub-gpui/src/run.rs:126,508`，每处后面紧跟一次 `register_engine`（单引擎）。连续性靠 `restore_session`（`crates/alva-hub-desktop/src/run.rs:530-537`）从 store 再水合，**不靠 hub-core 的 session 表**。

推导链：一个每轮新建、只注册一个引擎、用完即弃的 `HubRuntime`，结构上不可能承载「从 A 切到 B 再切回 A」的跨轮状态。所以 desktop / gpui / server 的 switch 必然另有实现 —— 就是 store 那套。

### C. store 里那套才是真正在跑的 switch

- `crates/alva-hub-desktop/src/store/branch.rs:208` `request_engine_switch` —— 事务化、校验 target、拒绝 running 中改选项。
- `crates/alva-hub-desktop/src/store/turn.rs:769,782,798` —— 把源 branch 置 `status = 'parked'` 并写 `watermark_turn_rowid`（`:776-783` 的注释说明 watermark 必须排除 in-flight 的那一轮）。
- `crates/alva-hub-desktop/src/store/handoff.rs:20-24` —— `enum HandoffKind { Full, Delta }`。
- `crates/alva-hub-desktop/src/store/tests.rs:825` `parked_branch_records_watermark_and_builds_a_delta_handoff_on_return` —— 回到停泊 branch 时构造 delta handoff，有测试钉死。

这一套是持久的（SQLite）、可审计的、多 branch 的，且与 ADR-0002 的「Identity / Handoff」两节逐条对应。

### D. 死掉的那套还藏着两条额外缺陷

**D-1：单槽 previous_*，A→B→C 之后 A 的 native session 彻底丢失。**

`crates/alva-hub-core/src/runtime.rs:378-386`：

```rust
session.previous_engine_id = Some(session.engine_id.clone());
session.previous_engine_session_id = session.engine_session_id.clone();
session.engine_id = new_engine_id;
session.engine_session_id = None;
session.pending_handoff = true;
```

`previous_*` 是**单槽**：第二次 switch 会用 B 覆盖 A，A 的 native session id 无处可寻。而 `docs/adr/0002-reversible-multi-engine-fallback.md:33` 写的是「Returning to an engine resumes its parked native session when possible.」—— **直接冲突**。store 那套用多 branch 表满足了这条，hub-core 这套满足不了。

**D-2：`capabilities().resume == false` 的引擎跑不了第二轮。**

`crates/alva-hub-core/src/runtime.rs:438-442`：

```rust
match &existing.engine_session_id {
    Some(engine_sid) => {
        if !engine.capabilities().resume {
            return Err(HubError::ResumeUnsupported(request.engine_id.to_string()));
        }
        runtime_request.resume_session = Some(engine_sid.clone());
    }
```

一旦 session 里存下了 `engine_session_id`，不支持 resume 的引擎就**永久被拒**，不会退化成「起一个新的 native session」。第一个真正的第三方无状态引擎注册进来，就只能跑单轮。（今天不炸，只因为四个官方 adapter 都声明 `resume: true`，且证据 B 里的 `HubRuntime` 每轮都是新的。）

## 复现

**D-1（可执行）**：在 `crates/alva-hub-core/src/runtime.rs` 的 `#[cfg(test)]` 段（`:785` 后）加一条测试：

1. 注册三个 `FakeEngine`（模板见 `:1130`）为 `a` / `b` / `c`；`execute` 一轮拿到 `a` 的 native session id。
2. `switch_engine(pid, "b")` → `switch_engine(pid, "c")`。
3. 观察到：`hub.session(&pid).unwrap().previous_engine_id == Some("b")`，`a` 的 native session id 在快照中已不存在。
4. 期望（按 ADR-0002:33）：切回 `a` 时应能 resume 它停泊的 native session。

**D-2（可执行）**：

1. 写一个 `capabilities()` 返回 `resume: false` 的 `FakeEngine` 变体，注册后 `execute` 一轮（它在 `SessionStarted` 里给出 native session id）。
2. 用同一个 `ProductSessionId` 再 `execute` 一轮。
3. 观察到：`Err(HubError::ResumeUnsupported("..."))`。
4. 期望：退化为起一个新的 native session（`resume_session = None`），或至少由引擎自己声明「无状态」这一语义（见工单 15 的 `EngineDescriptor`），而不是永久拒绝。

**B（可肉眼确认）**：`cat crates/alva-hub-server/Cargo.toml` —— `[dependencies]` 中无 `alva-hub-core`。

## 工作

### 第一批 —— 先在 ADR 补一节（无代码改动，必须先做）

- 在 `docs/adr/0002-reversible-multi-engine-fallback.md` 新增一节 **「Authority — where switch is implemented」**，写清：
  - product session 身份、单轮流投影、engine registry 归谁；
  - branch / parked / watermark / Full-vs-Delta handoff 归谁；
  - 两者的边界在哪一行代码上可验证。
- 这一节是后面所有改动的验收依据。**在它落地之前不要动第二批。**

### 第二批 —— 拍板（二选一，需用户裁定）

**选项 1（评审倾向）：下线 hub-core 的 handoff 状态机，hub-cli 改消费 store。**

- hub-core 只保留：engine registry + 单轮流投影 + 会话身份（`ProductSessionId` ↔ `engine_session_id`）+ `restore_session`。
- 整块删除 `crates/alva-hub-core/src/runtime.rs` 的 `switch_engine`（`:351-388`）、`pending_handoff`（`:107,126,143,166,382,453-467`）、`previous_engine_id` / `previous_engine_session_id`（`:103-105,123-124,141-142,164-165,378-379`）、`pending_handoff_context`（`:129,144,383,460,491,766`）、`restore_failed_handoff`（`:752-767` 及 `:623` 的调用）、`HubRequest::with_handoff_context`。
- `crates/alva-hub-cli/src/main.rs:182,531,648` 与 `crates/alva-hub-cli/src/session_store.rs:35-41` 改为消费 store 的 branch/handoff 能力。
- 代价：hub-cli 需要拿到 store。**这会撞上 `scripts/ci-check-deps.sh:89` 对 hub-cli 的允许依赖列表**，需一并评估是让 hub-cli 依赖 `alva-hub-desktop`（层级上难看），还是把 store 先从 `alva-hub-desktop` 抽成独立 crate。**这一点必须在拍板时一起决定，不能留到实现阶段。**
- 顺带解决 D-1 / D-2：删掉的代码不会再有 bug。

**选项 2：上提 —— 让 hub-core 成为权威，产品改用它。**

- 需要 hub-core 引入 branch 表、fallback policy、watermark、Full/Delta handoff 语义。
- 与 `crates/alva-hub-core/src/AGENTS.md` 的定位段（「位于引擎 contract 之上、具体 adapter 之下」「不能引入 ... 任何具体 adapter 依赖」）以及 `scripts/ci-check-deps.sh:88`（hub-core 只许依赖 `alva-engine-runtime`）直接冲突 —— 持久化、policy、审计都要进来。
- 若选此路，必须同时修 D-1（`previous_*` 单槽 → 多 branch）与 D-2（resume 退化），且必须先改 `AGENTS.md` 的定位。

### 第三批 —— 按拍板结果落地

- 选项 1：删代码 + 迁 hub-cli + 更新 `crates/alva-hub-core/src/AGENTS.md` 的「逻辑」第 5/6/8 条与「业务域清单」。
- 选项 2：修 D-1 / D-2 + 三个产品改接线 + 更新 `AGENTS.md` 定位与 `ci-check-deps.sh` 规则。

## 验收

- [ ] `docs/adr/0002-reversible-multi-engine-fallback.md` 存在 Authority 一节，明确点名权威层与被下线/被上提的那一层，并给出可验证的代码位置。
- [ ] 选项 1 路径：`rg -n 'switch_engine|pending_handoff|previous_engine_id|restore_failed_handoff|with_handoff_context' crates/alva-hub-core/` 输出为空。
- [ ] 选项 1 路径：`alva-hub` CLI 的 handoff 端到端行为不回归 —— `cargo test -p alva-hub-cli` 全绿，且 `crates/alva-hub-cli/src/handoff.rs` 现有两条测试（`:54`、`:63`）保留。
- [ ] 选项 2 路径：新测试 `switch_a_to_b_to_c_then_back_to_a_resumes_a_parked_native_session` 通过（钉死 ADR-0002:33）。
- [ ] 选项 2 路径：新测试 `stateless_engine_starts_a_fresh_native_session_instead_of_failing` 通过（钉死 D-2 退化行为）。
- [ ] `crates/alva-hub-core/src/AGENTS.md` 的「逻辑」与「约束」段与落地结果一致（逐条肉眼对照，不留描述已下线机制的条目）。
- [ ] `bash scripts/ci-check-deps.sh` 全绿；若规则有改动，改动在 commit message 中说明理由。
- [ ] `cargo test --workspace` 全绿。

## 依赖与切分

- **前置**：无硬前置。第一批（ADR 补节）可立刻开工，且**必须先于其余批次**。
- **与 13 的关系**：可并行。13 动 `runs` 表与控制面，14 动 handoff 状态机，两者在 `runtime.rs` 中的改动区间不重叠（13 集中在 `:483-680`，14 集中在 `:351-388` + `:436-467` + `:752-767`）；若同期进行，约定 14 先合，13 在其上 rebase。
- **对 15 的约束**：**14 必须先拍板**。否则工单 15 的 `EngineDescriptor` 会被塞进一个待下线的状态机，白搬一次。
- **切分点**：
  1. ADR 补节（纯文档，当天可交付，且独立有价值）；
  2. 拍板会（含 `ci-check-deps.sh` 依赖走向的决定）；
  3. 若选选项 1：先删 hub-core 死代码但**保留** hub-cli 的现有行为（临时在 hub-cli 内联那一小段状态机），再单独一个 commit 迁到 store —— 两步都可独立回滚；
  4. 若选选项 2：D-1 与 D-2 各自一个可独立验证的 commit，产品接线单独一个。
