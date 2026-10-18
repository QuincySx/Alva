# 横向支撑 · Strict transport JSON
> 本地产品复用的JSON歧义与资源边界，不拥有业务schema或网络执行。

## 地位
纯 JSON helper，可由 Hub HTTP、Gateway、入口配置与 portable extension Tools 消费；只依赖标准库与 wire，不拥有 native 资源，SDK 不反向依赖。
## 逻辑
Validate检查完整object/Unicode/重复键/64层深度，保留未知protocol字段供codec诊断。
ValidateObject额外按具体DTO精确字段名检查，拒绝root null、大小写别名与未知字段。
## 约束
调用方先限制字节与并发，再调用；不读取文件/HTTP、不存储payload或秘密。
校验通过不是授权/必填业务值通过；后续类型解码和领域验证仍须执行。
## 业务域清单
| 文件 | 职责 |
|---|---|
| validate.go | strict object/DTO检查与有界递归token walker |
| validate_test.go | Unicode/重复键/大小写/null/深度与未知protocol字段保留 |
