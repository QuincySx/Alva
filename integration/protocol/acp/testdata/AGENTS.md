# 横向支撑 · ACP schema fixture
> 官方schema的测试期结构投影，不进入runtime binary或模型上下文。

## 地位
来源：agentclientprotocol/agent-client-protocol，Apache-2.0，commit `00c5a02d7ab96b9d6aa214002d408ce9d972b4e4`。

## 逻辑
`schema/v1/schema.json`中本客户端使用的14个根定义及112个递归可达定义；仅移除description/title/default/discriminator/x-docs注释。
所有validation关键词/属性名保留，测试仅验证所支持JSON-Schema关键词，不宣称实现完整2020-12引擎。

## 约束
固定版本离线验收；测试不联网、不随main静默升级，不含旧custom envelope或v2转换。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Official projection | `schema-v1.json` | initialize/session/prompt/permission/update所需官方结构 |
| License | `LICENSE` | 官方Apache-2.0原文，保留投影来源与许可 |
