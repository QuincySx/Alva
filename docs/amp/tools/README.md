# 工具系统目录

Amp 的工具（Tool）系统是整个 agent 的手脚。本目录文档覆盖工具的**架构、数据结构、调度、清单**四层。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`architecture.md`](./architecture.md) | 工具数据结构 + Observable fn + preprocessor 钩子 |
| [`execution-scheduler.md`](./execution-scheduler.md) | 资源锁、并发策略、HITL 审批拦截 |
| [`catalog.md`](./catalog.md) | 40+ 工具清单 + 按功能分组 |
| [`custom-agents.md`](./custom-agents.md) | `.agents/agents/*.md` 自定义子 agent |

## 核心数据结构一览

```js
ToolDefinition = {
  spec: {
    name:             <string 或符号引用>,
    description:      <string，大段 markdown>,
    inputSchema:      <JSON Schema 或 zod.toJSONSchema() 输出>,
    source:           "builtin" | {toolbox: path} | "mcp-workspace" 
                    | "mcp-global" | "mcp-flag" | "mcp-other" 
                    | "plugin" | "other",
    meta?:            { disableTimeout?: boolean, ... },
    executionProfile: {
      resourceKeys: (args) => [{ key: string, mode: "read" | "write" }],
      serial?:       boolean    // true = 全局独占
    }
  },
  fn: (args, ctx) => Observable<{ status, progress?, result?, error? }>,
  preprocessArgs?: (args, ctx) => args    // 可选预处理
}
```

## 工具来源分层

`amp tools` CLI 按以下顺序打印：

```
1. builtin           ← Amp 二进制内置
2. mcp-workspace     ← <workspace>/.amp/settings.json 配置的
3. mcp-global        ← 用户全局 settings 配置的
4. mcp-flag          ← 启动时 --mcp-server 传入的
5. mcp-other         ← 其他 MCP 来源
6. toolbox           ← .agents/agents/*.md（自定义子 agent）
7. plugin            ← .amp/plugins/*.ts 注册的
8. other             ← fallback
```

每层可以覆盖前一层的同名工具。

## 数字速查

| 值 | 含义 |
|---|---|
| ~18 | builtin 工具默认数量（估计，不含 Aggman/Librarian 变体） |
| ~26 | 一个中等规模 MCP server 的工具数（示例：chrome-devtools） |
| ~17,700 tokens | 26 个 MCP 工具 spec 加起来的 token 占用（Amp 的 skill 指南原话） |
| 90%+ | 通过 `includeTools` 过滤能省的 token 比例 |
