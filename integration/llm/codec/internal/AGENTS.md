# LLM Codec · Private implementations
> 只供上级 codec 使用的第三方转换实现。

## 地位
内部实现容器，不向 SDK、Provider、Gateway 或产品直接公开 IR。
## 逻辑
上级 Exchange 调用 magpie 的请求解析/构建和响应编码；原生解码、限额、终态与损耗检查留在 codec。
## 约束
不引入产品 runtime、HTTP、文件、账号、环境或执行权限；具体来源与修改随子目录记录。
## 业务域清单
| 目录 | 职责 |
|---|---|
| `magpie/` | 固定 MIT 上游源码的纯转换切片与 Alva 私有适配 |
