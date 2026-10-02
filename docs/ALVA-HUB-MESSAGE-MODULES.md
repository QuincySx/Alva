# Hub 消息模块 v1

消息模块是可信 Hub 呈现扩展，不是可执行消息、SDK Plugin 或远程插件安装器。
正文来源仍为权威 assistant 文本/工具全文，UI 原文保持可读/可复制。注册 renderer
不改变 RuntimeEvent、kernel 或 agent-core。

## 生产者

助手在 `alva-display` fenced code block 中提供 JSON；工具可直接返回同一 JSON，
也可经既有 `ToolOutput.content` text blocks 包装。只对完整的单页工具结果呈现，
不把截断预览或多页片段当完整模块。例：

```json
{"schema":"alva.display","version":1,"modules":[
  {"type":"alva.table","version":1,"data":{"title":"结果","columns":["项目","数量"],"rows":[["通过",3]]}},
  {"type":"alva.chart","version":1,"data":{"title":"趋势","style":"line","points":[{"label":"A","value":2},{"label":"B","value":5}]}},
  {"type":"alva.diagram","version":1,"data":{"title":"流程","nodes":[{"id":"a","label":"输入"},{"id":"b","label":"处理"}],"edges":[{"from":"a","to":"b","label":"按需"}]}}
]}
```

## 注册新展示能力

在可信应用装配处注册，不从消息/URL 下载或 eval renderer：

```tsx
const registry = createMessageModuleRegistry([
  ...builtinMessageModuleRenderers,
  { type: "example.metric", version: 1,
    validate: (data) => /* validate a bounded, inert payload */ false,
    render: (data) => /* trusted React component */ null },
]);
<MessageModuleProvider registry={registry}><App /></MessageModuleProvider>
```

实际 ChatTranscript 的助手代码块和 ToolContent 完整结果都消费这个 provider。
同 type/version 重名 fail-fast；逐模块验证/渲染异常有边界，未知类型或非法数据
显示不可用并保留 JSON。普通/部分 JSON 和未知 envelope version 原样显示。

## 限额与安全

envelope ≤16384 JS 字符、1–8 个模块。内置表格 ≤12 列/100 行；chart 为 bar/line，
≤100 个有限数值点（绝对值 ≤1e12），含可读数据表；diagram ≤30 个唯一节点/
60 条有效边，图和文字列表同时保留。标签 ≤500 字符，数据不是 HTML/JS/URL。
不执行 Mermaid 或远程脚本，不自动加载外部图像。扩展 renderer 自己的验证必须
限制其结构和资源用量。

图片/视频附件采用 session artifact 引用，不把媒体字节塞入 display envelope 或
模型上下文。未来生成图像/压缩视频/抽帧产物必须沿 ADR-0011 的处理清单与权限路径。
本协议不是图像生成服务，也不声称已有第三方动态安装生态。
