# alva-protocol-skill
> Skill 协议层，负责技能的发现、加载、存储与 prompt 注入

## 地位
技能系统的协议实现。定义 Skill 数据模型和仓库接口，提供渐进式加载（SkillLoader）、prompt 注入（SkillInjector）、内存缓存（SkillStore）和文件系统仓库（FsSkillRepository）。被 alva-app-core 的 skills 模块集成使用。

## 逻辑
```
FsSkillRepository (磁盘) ──→ SkillLoader (渐进式加载)
                                  │
                                  ▼
                             SkillStore (内存缓存)
                                  │
                                  ▼
                           SkillInjector (prompt 注入)
                                  │
                                  ▼
                            system prompt 中的技能上下文
```
- `SkillRepository` trait 定义技能仓库接口（list / get / install / uninstall）
- `SkillLoader` 支持渐进式加载，按需加载技能详情
- `SkillMeta::invocation` 控制目录可见性：`auto` 常驻目录、`explicit` 仅精确点名
- `SkillInjector` 将已选中的技能按 Auto / Explicit / Strict 策略转换为 prompt 片段
- `SkillStore` 提供线程安全的内存缓存

## 约束
- Skill 类型是纯数据结构，不包含执行逻辑
- FsSkillRepository 假定特定的目录布局（每个 skill 一个目录 + manifest 文件）
- SkillInjector 输出纯文本，不负责 prompt 格式化策略

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 类型定义 | `types.rs` | Skill、SkillMeta、SkillInvocation、SkillBody、SkillKind 等核心类型 |
| 仓库接口 | `repository.rs` | SkillRepository trait（list / get / install / uninstall） |
| 加载器 | `loader.rs` | SkillLoader 渐进式加载逻辑 |
| 注入器 | `injector.rs` | SkillInjector 将技能转换为 prompt 片段 |
| 缓存存储 | `store.rs` | SkillStore 线程安全内存缓存 |
| 文件系统仓库 | `fs.rs` | FsSkillRepository 基于文件系统的仓库实现 |
| 错误 | `error.rs` | 技能系统错误类型 |
