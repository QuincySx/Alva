# Token generator
> 把 tokens.json 生成为 tokens_gen.go 的确定性小程序。

## 地位
`go generate ./apps/ui/kit/theme` 的执行体，只在开发时运行；Go 构建不依赖它，也不依赖 Python。
## 逻辑
读当前目录的 `tokens.json`，按 token 类型输出 Palette 字段、无类型常量或 Narrow 函数，经 `go/format` 写出。
## 约束
纯标准库，不导入 MyGo；未知 token 类型、版本或颜色格式一律报错，不静默跳过。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | 解析、生成与写出 |
| gen_test.go | `TestTokensGoMatchesJSON`（提交的 Go 等于生成结果）与生成器错误路径 |
