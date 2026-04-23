# MCP Resource Reading

MCP 协议除了 tools 还有 **resources**（URI 寻址的可读内容，典型：文件系统 server 的 `file:///...`、数据库 server 的 schema 文档）。Amp 把 resource 读取封装成一个**独立 builtin tool** `read_mcp_resource`。

## Tool spec（反编译还原）

```js
// from strings.txt:65837~65838
{
  spec: {
    name: "read_mcp_resource",
    description: `Use when the user references an MCP resource, e.g. "read @filesystem-server:file:///path/to/document.txt"`,
    inputSchema: {
      type: "object",
      properties: {
        server: {
          type: "string",
          description: "The name or identifier of the MCP server to read from"
        },
        uri: {
          type: "string",
          description: "The URI of the resource to read"
        }
      },
      required: ["server", "uri"]
    },
    source: "builtin"
  },
  fn: a1R,  // implementation
}
```

两个参数：`server` 名字 + resource `uri`。返回 text。

## 调用触发模式

文档原话的 "when to use"：

> Use when the user references an MCP resource, e.g. `"read @filesystem-server:file:///path/to/document.txt"`

所以用户在 prompt 里的语法：

```
@<server>:<uri>
```

用户写 `@filesystem-server:file:///path/to/document.txt`，Amp 的 prompt 预处理不会自动解析，而是把这个提示留在 prompt 里，靠 `read_mcp_resource` 的 description 教模型自己去调。

## 实现（反编译）

```js
// from strings.txt:64687
... [Resource content truncated - showing first ${Math.round($z/1024)}KB of ${c}KB total.
     The resource was too long and has been shortened.]

return {status: "done", result: i || "[Empty resource]"};

// error path:
catch (a) {
  throw Error(`Failed to read resource "${t.uri}" from MCP server "${t.server}": ${a.message}`);
}
```

- 截断阈值变量 `$z`，确切值反编译里没露出（上下文里的其它 truncation 常量 `FL` 对应 tool output，`$z` 似乎专门给 resource 用）。根据 Amp 一贯 128KB/文件的保守取值习惯，`$z` 也应在 `65536 ~ 131072` 之间。
- 空内容返回占位 `"[Empty resource]"` 而不是报错。
- 错误一律包成 `Error("Failed to read resource ...")`。

## 区别于 `read_file`

| 维度 | `Read` (builtin) | `read_mcp_resource` |
|---|---|---|
| 寻址 | 绝对文件路径 | `(server, uri)` 组合 |
| Transport | 本地 FS | MCP protocol |
| 截断 | `read_range[500, 1000]` 行范围 | 字节预算（$z） |
| 用途 | 代码库文件 | 跨系统文档（DB schema、API specs） |

Amp 没给 `read_mcp_resource` 加 range 参数，要就全拿，太长就头部截断。这合理：resource 不假定是 line-oriented（可能是图像 base64、protobuf 描述等）。

## Resource listing

反编译里**没有**找到 `list_mcp_resources` 或 `ListResources` call。猜测 Amp 的策略：

- resources 不主动 list 进 prompt（列 URI 就很烧 token）
- 靠用户在 prompt 里用 `@server:uri` 明确引用
- 模型不知道有哪些 resources 就是不知道，等于让用户做 explicit source pointer

这是"context 省钱"的一部分。MCP 协议层的 `ListResources` 支持仍然在（MCP SDK 自动做），只是 Amp 主动不暴露给模型。

## 对 Alva 的启发

当前 `alva-protocol-mcp/src/resources.rs`（127 行）已有协议层实现。缺的是**暴露成 engine tool**：

1. **新建 builtin tool**：在 `alva-extension-builtin` 或新 `alva-extension-mcp` crate 里：

   ```rust
   // crates/alva-extension-mcp/src/read_resource.rs
   pub struct ReadMcpResourceTool {
       client: Arc<McpClient>,
   }
   
   #[async_trait]
   impl Tool for ReadMcpResourceTool {
       fn name(&self) -> &str { "read_mcp_resource" }
       fn parameters_schema(&self) -> Value {
           json!({
               "type": "object",
               "properties": {
                   "server": { "type": "string", "description": "MCP server id" },
                   "uri":    { "type": "string", "description": "Resource URI" }
               },
               "required": ["server", "uri"]
           })
       }
       async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
           let server = input.get("server").and_then(|v| v.as_str()).ok_or(...)?;
           let uri = input.get("uri").and_then(|v| v.as_str()).ok_or(...)?;
           let content = self.client.read_resource(server, uri).await.map_err(...)?;
           let (truncated, msg) = truncate_text(&content.text, MAX_RESOURCE_BYTES);
           // 如果有截断，附加 "... [Resource content truncated - showing first NKB of MKB total.]"
           Ok(ToolOutput::text(truncated))
       }
   }
   ```

2. **`MAX_RESOURCE_BYTES` 选 `131072`**（128 KB，和 Amp `read_web_page` 截断一致）。

3. **URI 的 @-引用预处理**（可选但建议）：在 user prompt 预处理里把 `@<server>:<uri>` 模式识别出来插到 `<context>` tag 里，而不是等模型去调。Amp 没做这个，但 Alva 有 `kernel` 层的 handoff 能力更适合预处理。

4. **不实现 `list_mcp_resources` tool**。跟 Amp 一样，listing 交给用户 explicit 引用。好处：
   - 省 token（一个大 server 可能有几千 resources）
   - 符合 MCP 设计：resource 是地址化的，不是目录化的
   - 避免模型"看到就想读"的贪婪行为

5. **不要**在 `McpClient::connect()` 时 eager list resources。Amp 不做，Alva 也不该做。按需调用 `read_resource(server, uri)`。

此外，Alva 已有的 `elicitation.rs` 和 `prompts.rs` 也都是 MCP spec 的一部分，但 Amp 对 prompts 也没暴露成 tool（没看到 `use_mcp_prompt` 之类的工具）。这条线可以先放着，等真有用户需求再加。
