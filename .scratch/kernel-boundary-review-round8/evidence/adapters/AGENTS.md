# Adapter evidence
> 通过真实 adapter execute 复现成功误判。

## 地位
独立 Cargo package，不修改生产文件。
## 逻辑
本地假 CLI 输出完整成功序列；收集真实事件并比较正文/错误与 Normal harness 判断。
## 约束
仅本地 fixture；无网络、密钥、真实 CLI。Cargo.lock 固定独立依赖解析。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 包 | Cargo.toml / Cargo.lock | 独立探针依赖 |
| 探针 | main.rs | 实际运行两个 adapter 的 execute |
| fixture | fake-codex.sh / fake-pi.sh | 本地成功 JSONL |
| 结果 | RESULTS.md | 已执行命令与精简输出 |
