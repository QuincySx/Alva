# Icon name generator
> 把 icons.json 生成为 names_gen.go 的确定性小程序。

## 地位
`go generate ./apps/ui/kit/icon` 的执行体，只在开发时运行。
## 逻辑
读当前目录的 `icons.json`，输出设计顺序的 `Name` 常量、Lucide 文件名映射与顺序表，再附加派生的 `PanelLeft`。
## 约束
纯标准库；空名称或重复名称报错。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | 解析清单并写出 names_gen.go |
