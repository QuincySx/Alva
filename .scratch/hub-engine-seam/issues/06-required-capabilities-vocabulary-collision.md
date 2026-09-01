# 06 required_capabilities 词表对撞：两套互不相交的词汇挤在同一个字段里

- 级别：P0
- 影响：一旦有任何模型能力词（`images` / `tools` / `vision` …）进入引擎目标的 `required_capabilities`，该目标就会在降级链里被永久静默否决——用户看到的是一句「target lacks required capabilities: images」，而 `images` 本来就不属于引擎能力词表，永远不可能被满足。
- 状态：TODO

**这是埋着的雷，不是正在爆的雷。** 出厂 UI 路径今天一律写 `capabilities: []`（证据见下），所以现在触发不了。修在数据被填充之前是几行改动；等这批数据被填满之后再修，就是一次线上事故加一次数据迁移。

## 证据

### 字段本身：无命名空间、无校验的 `Vec<String>`

- `crates/alva-hub-gpui/src/store.rs:38` —— `pub required_capabilities: Vec<String>,`
- `crates/alva-hub-desktop/src/store/branch.rs:30` / `:107` —— 同样是裸 `Vec<String>`
- `crates/alva-hub-desktop/src/store/mod.rs:169` —— 直接 `serde_json::to_string(&target.required_capabilities)` 落库；`crates/alva-hub-desktop/src/store/db.rs:348` 加的列是 `required_capabilities_json TEXT NOT NULL DEFAULT '[]'`。字符串进、字符串出，中间没有任何枚举收口。

### 写入端 A：模型词表

- `crates/alva-hub-desktop/src/providers.rs:666` —— `target.required_capabilities = model.capabilities.clone();`
- `crates/alva-hub-desktop/src/providers.rs:33` / `:81` —— `model.capabilities` 的类型就是 `pub capabilities: Vec<String>`，来源是磁盘上的 provider catalog 与 webview 传入的自由字符串，**没有任何白名单**。
- `crates/alva-hub-gpui/src/models.rs:395-401` ——

  ```rust
  let mut required_capabilities = vec!["streaming".to_string()];
  if model.capabilities.images {
      required_capabilities.push("images".to_string());
  }
  if model.capabilities.tools {
      required_capabilities.push("tools".to_string());
  }
  ```

  `"streaming"` 恰好两套词表都有，`"images"` / `"tools"` 只属于模型词表。
- `crates/alva-hub-gpui/src/models.rs:414` —— 上述 vec 直接写进 `EngineTarget.required_capabilities`；调用点在 `crates/alva-hub-gpui/src/ui/app.rs:973`、`:1718`、`:1866`、`:1903`、`:1949`——**全是主路径的新会话/换模型入口**。

### 读取端 A：引擎标签词表，只有 5 个词

- `crates/alva-hub-desktop/src/readiness.rs:251-268` `capability_labels` —— 穷尽产出且只产出 `streaming` / `tool_control` / `permission_callback` / `resume` / `cancel`。
- `crates/alva-hub-gpui/src/readiness.rs:233` —— 同一份实现的副本。
- 两个词表的交集只有 `streaming` 一个词。

### 读取端 B：模型词表也真有消费者（所以这不是「写错字段」那么简单）

- `crates/alva-hub-desktop/src/alva.rs:235-237` ——

  ```rust
  images: target
      .required_capabilities
      .iter()
      .any(|capability| matches!(capability.as_str(), "images" | "vision")),
  ```
- `crates/alva-hub-gpui/src/alva.rs:200-202` —— 同样按 `"images"` 判断。

**同一个字段被两个消费者按两套词表读**：Pi/Alva 的 provider config 组装期待模型词，run.rs 的降级否决期待引擎词。这才是根因——不是某处填错了，是这个字段从来没有定义过它到底是哪套词表。

### 校验端形同虚设

- `crates/alva-hub-gpui/src/store.rs:3171-3180` —— 只拒空串：

  ```rust
  if target
      .required_capabilities
      .iter()
      .any(|capability| capability.trim().is_empty())
  ```
- `crates/alva-hub-desktop/src/store/mod.rs:136-145` —— 逐字相同的一段。
- 两处都对 `engine_id`（`EngineTarget::for_engine`）和 `reasoning_effort`（desktop `:146-151` 的显式白名单）做了收口，**唯独 capability 没有**。

### 后果：降级链上的永久静默否决

- `crates/alva-hub-desktop/src/run.rs:328-351` ——

  ```rust
  let available = capability_labels(&engine.capabilities());
  let missing: Vec<String> = candidate
      .required_capabilities
      .iter()
      .filter(|capability| !available.contains(capability))
      .cloned()
      .collect();
  if !missing.is_empty() {
      let veto = format!("target lacks required capabilities: {}", missing.join(", "));
      store.record_fallback_skip(...)?;
      ...
      continue;
  }
  ```
- `crates/alva-hub-gpui/src/run.rs:319-342` —— 同一段的副本。
- `available` 恒为 5 词词表的子集，因此任何模型词都必然落进 `missing`，`continue` 之后该候选**这一轮被跳过，且每一轮都会被同样跳过**——目标一旦带上模型词，就是永久否决。用户在时间线上只看到一句他无法理解的 skip 理由。

补充一条重要边界：**这个否决只作用在降级候选上，主目标不受检查。** `crates/alva-hub-desktop/src/run.rs:230-236` 主目标直接 `build_engine` 然后 `execute_persisted_turn`，全程没有 capability 比对。所以症状是「主引擎跑得好好的，降级链却神秘地跳过所有目标」。

### 今天还没爆的原因

- `crates/alva-hub-desktop/src/providers.rs:1165-1178` `default_model` —— `capabilities: Vec::new()`，出厂模型条目能力为空。
- `crates/alva-hub-desktop/web/src/components/provider-settings.tsx:240` / `:395` / `:405` / `:1025` —— 全部 `capabilities: []`，webview 新建/编辑模型时也不填。

也就是说 `providers.rs:666` 今天 clone 到的永远是空 vec。**只要有人给出厂 catalog 补一条 `capabilities: ["images"]`，或在 provider-settings 里加上能力勾选 UI，雷就炸。** gpui 的 `models.rs:395-401` 更近一步——它已经在无条件 push `"streaming"` 了，只要模型元数据里 `images`/`tools` 为真就会立刻写入非法词。

### 测试夹具用的是一个根本不存在的能力名

- `crates/alva-hub-desktop/src/store/tests.rs:176` —— `required_capabilities: vec!["streaming".into(), "tool_calls".into()],`
- `crates/alva-hub-desktop/src/store/tests.rs:760` —— `required_capabilities: vec!["tool_calls".into()],`
- `crates/alva-hub-desktop/src/store/tests.rs:1117` —— `claude.required_capabilities = vec!["streaming".into(), "tool_calls".into()];`

`"tool_calls"` **两套词表里都没有**（引擎侧真名是 `tool_control`，见 `readiness.rs:255`；模型侧 gpui 用的是 `tools`）。这三处夹具永远绿，因为它们只走 store 的存取与校验，不经过 `run.rs` 的否决逻辑。同类污染还有：

- `crates/alva-hub-desktop/src/providers.rs:1276` —— `capabilities: vec!["vision".into()]`
- `crates/alva-hub-desktop/src/run.rs:1575` —— `required_capabilities: vec!["tools".into()]`，但它是**主目标**，按上面那条边界永远不会被检查，所以也永远绿。

## 复现

静态可证（今天的出厂数据触发不了，推导链如下）：

1. 令 `T` 是一个降级候选，`T.required_capabilities` 含任一模型词，例如 `"images"`。
2. `crates/alva-hub-desktop/src/store/mod.rs:136-145` 的 `validate_engine_target` 只拒空串，`"images"` 非空 → 校验通过，落库。
3. 降级触发，`crates/alva-hub-desktop/src/run.rs:328` 算出 `available = capability_labels(...)`；由 `readiness.rs:251-268` 的穷尽实现可知 `available ⊆ {streaming, tool_control, permission_callback, resume, cancel}`，其中不含 `"images"`。
4. `:329-334` → `missing = ["images"]` → `:335` 条件成立 → `:338-344` `record_fallback_skip` → `:350` `continue`。
5. 该候选被跳过。因为第 3 步的结论与运行时状态无关，**每一次降级都会得到同样结果**。

可执行复现（当前代码即可）：

1. 在 `crates/alva-hub-desktop/src/store/tests.rs:1117` 那种夹具的基础上，构造一条 fallback 链，候选带 `required_capabilities: vec!["images".into()]`。
2. 走 `run_persisted_with_target`，让主目标失败以触发降级。
3. 观察到：时间线出现 `... skipped — target lacks required capabilities: images`，降级链一路走空到 `record_fallback_exhaustion`。
4. 期望：`"images"` 这类模型词不参与引擎能力否决；引擎能力不足时给出的词必须来自 `capability_labels` 的 5 词词表。

## 工作

- 给这个字段定一套单一词表并落到类型上。两条路线二选一，先拍板再动手：
  - **路线 A（推荐，改动小）**：明确 `required_capabilities` 只表示**引擎能力**，用一个 `EngineCapability` 枚举（`streaming` / `tool_control` / `permission_callback` / `resume` / `cancel`）替换 `Vec<String>`，序列化保持现有 JSON 字符串形状以兼容已有库；模型能力另开字段（如 `model_capabilities: Vec<String>`），把 `alva.rs:235-237` 与 `alva.rs:200-202` 的 `images`/`vision` 读取迁过去。
  - **路线 B（改动更小但更弱）**：保留 `Vec<String>`，在 `validate_engine_target`（`crates/alva-hub-gpui/src/store.rs:3157`、`crates/alva-hub-desktop/src/store/mod.rs:122`）加白名单，非法词直接拒绝写入，仿照 desktop `:146-151` 对 `reasoning_effort` 的写法。缺点是 `alva.rs` 那条模型词读取会立刻失效，必须一并搬走。
- `crates/alva-hub-desktop/src/providers.rs:666` —— 不再无条件 clone 模型词表；按选定路线改成写模型能力字段，或做一次映射 + 丢弃非法词并 warn。
- `crates/alva-hub-gpui/src/models.rs:395-401` —— 同上，`"images"` / `"tools"` 不再进 `required_capabilities`。
- `crates/alva-hub-desktop/src/run.rs:336-337`、`crates/alva-hub-gpui/src/run.rs:327-328` —— veto 理由文案带上「引擎可用能力清单」，让用户看到的不是一个孤零零的词。
- 修正污染的夹具：`crates/alva-hub-desktop/src/store/tests.rs:176` / `:760` / `:1117` 的 `"tool_calls"` → `"tool_control"`；`crates/alva-hub-desktop/src/providers.rs:1276` 的 `"vision"`、`crates/alva-hub-desktop/src/run.rs:1575` 的 `"tools"` 一并按新契约调整。
- 在 `crates/alva-hub-desktop/AGENTS.md` / `crates/alva-hub-gpui/AGENTS.md` 记一句：这个字段属于哪套词表、新增词汇改哪里。

## 验收

- [ ] 新增一条**穿透 `run.rs` veto** 的测试：fallback 候选带 `required_capabilities: vec!["tool_calls".into()]`，断言它被 `record_fallback_skip` 跳过。这条测试在当前代码上必须**通过**（证明雷存在），并让 `store/tests.rs:176` / `:760` / `:1117` 的夹具在改词表后立刻变红——即修完之后，这三处夹具不改就编译失败或断言失败。
- [ ] 新增测试：fallback 候选带 `required_capabilities: vec!["images".into()]`，断言它**不再**被否决（修复后），且模型的 images 能力仍然正确传到 `crates/alva-hub-desktop/src/alva.rs:235` 组装出的 provider config 里（`images == true`）。
- [ ] 新增测试：`validate_engine_target` 拒绝一个不在词表内的能力名（例如 `"tool_calls"`），错误信息里列出合法词表；`rg -n 'tool_calls' crates/alva-hub-desktop/src --glob '*.rs'` 输出为空。
- [ ] 新增测试：断言 `capability_labels` 产出的集合与新词表定义**恰好相等**（防止两边再次漂移）。
- [ ] `rg -n 'required_capabilities' crates/ --glob '*.rs'` 的每一处写入点，其词汇来源在 PR 描述中逐条标注属于哪套词表。
- [ ] `cargo test -p alva-hub-desktop -p alva-hub-gpui` 全绿。
