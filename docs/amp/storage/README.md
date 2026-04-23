# Storage 目录

> Amp 的线程持久化和同步策略。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`thread-model.md`](./thread-model.md) | Thread + Message 的数据结构 |
| [`sync-protocol.md`](./sync-protocol.md) | Server-side 存储 + version vector + flushVersion |

## 最核心的事实

**Amp 的 thread 数据不在本地。** `~/.amp/` 只存 settings、plugins、secrets、CLI 历史 —— 没有 thread 正文。

所有 thread CRUD 都是 HTTP 调用到 ampcode.com 后端：

```
~/.amp/
├── bin/amp
├── settings.json           ← 用户全局配置
├── plugins/*.ts             ← 插件源码
└── (其他 OAuth token 等 secrets 在 keychain)

<workspace>/.amp/
├── settings.json            ← 工作区配置（含 MCP server 列表）

<workspace>/.agents/
├── skills/                  ← 工作区 skills
└── agents/                  ← 自定义子 agent 定义
```

**Thread 数据**（messages, tool calls, usage, 等）全部在 server。
