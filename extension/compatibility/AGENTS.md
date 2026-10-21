# L4 · Optional convention compatibility
> 显式选择项目规范，将上下文、Skill 与 Agent 文件归一到既有扩展插口。

## 地位

外围可选 Plugin，默认不安装；不改变 SDK、默认 harness、授权或执行模型。

## 逻辑

`OpenFile(ctx, absoluteWorkspace, relativeConfig)` 只读明确的 JSON，或程序直接提供 `Config`。
规则只沿 workspace → scope 的祖先链，按外到内、同目录 rules 数组顺序贡献系统提示。
SKILL.md 先解析冻结在内存，模型仅见目录，点名才交正文，资源单独按需读取。
Agent Markdown 转成现有 subagent.TemplateRegistry；宿主显式把它交给现有子代理 Plugin。
`Snapshot.Plugin(session, refs, base)` 将可选 native Skill 仓库合为一个目录/一个 skill Tool；
按类型/name 判重；同名且规范化定义/资源完整相同只注册一份，差异报出双方来源；不能再注册另一 Skills Plugin。

## 约束

没有 HOME/上级目录/全盘发现，不创建/安装/执行文件，不展开 @import 或 include。
显式 scope 是本轮选择，不是随任意工具访问自动切换目录；其他目录的规则未加载。
规则每文件64KiB/总提示256KiB；配置32KiB；Skill至多1024个/规范化正文总32MiB；
Agent至多64个、prompt16KiB。目录/文件拒绝 symlink 与逃逸，打开时核对 identity。
项目规则、Skill 文档与模板本轮冻结；SHA 绑定配置/规则字节/规范化 Skill Document/模板字节，
不包含每次按需读取的资源数据。资源是数据，不执行；下一轮重新打开并形成新指纹。
重复的同路径/同格式来源只读一次；副本按工具名集合/完整元数据/正文比较，同名 Skill 还比较资源路径、媒体类型和字节 SHA。只有发生重复才做宿主冷资源哈希，每个合并阶段共64MiB，来源回执最多1MiB；正文不因此投递模型。等价副本优先现有 native 仓库，其余用显式来源顺序首项；本轮资源读取使用主来源当前数据，下一轮重新校验。不同逻辑名不按正文哈希合并；不同目录的规则保留作用域和顺序。
资源 owner 在 Agent join 后 Close；durable 恢复校验同指纹，修改指令须恢复原配方或审阅放弃。
allowed-tools 只作请求元数据；模板工具名只在直接父真实实例/显式 binder 范围内可委派。
Claude 仅支持 name/description/tools/model: inherit 和正文；缺失 tools 表示无宿主工具，
不继承全部父能力。Read/Write/Edit/Grep 映射到 Alva 同类文件工具，其他精确名由实际宿主能力决定；
Glob/Bash 等厂商能力不伪装成另一工具。Hook、permissionMode、独立 model 等字段拒绝。
条件 frontmatter 拒绝；CLAUDE.md/GEMINI.md 是选定目录内的 Markdown 子集，不宣称完整厂商配置兼容。

## 业务域清单

| 文件 | 职责 |
|---|---|
| config.go | 精确 JSON、显式格式/相对路径/预算与来源配置 |
| files.go | pinned 逐层目录、regular UTF-8 有界读取与 identity 校验 |
| agents.go | Claude 基础声明归一现有 Template；不支持的执行政策报错 |
| snapshot.go | 本轮冻结、来源 receipt/指纹、祖先规则顺序和 owned descriptors |
| plugin.go | 既有系统提示贡献、单一渐进 Skill 工具与 native catalog 合并 |
| merge.go / merge_test.go | name/规范化定义/完整资源的等价合并、不同内容双方来源冲突、native主来源与closed缓存拒绝；不合并权限 |
| compatibility_test.go | 实际 SDK 请求/Tool、范围/覆盖顺序、冻结/指纹、路径/权限与冲突回归 |
