# Remote Runtime 目录

> Amp 的两个"远程/外部执行"机制：DTW 云端执行、Stream-JSON subprocess IPC。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`dtw.md`](./dtw.md) | Distributed Thread Worker（跑在 Cloudflare Workers 上的远程 executor） |
| [`stream-json.md`](./stream-json.md) | `--execute --stream-json` subprocess NDJSON 协议 |

## 背景

Amp 在 CLI/IDE 外，还有**两套额外的运行形态**：

1. **DTW** —— 用户在 ampcode.com web UI 启动的 remote execution thread，跑在 Cloudflare Workers 上。本地 CLI 可以 "attach" 到 DTW thread，或者 DTW 自己独立跑。

2. **Stream-JSON** —— `amp --execute --stream-json` 把 Amp 变成一个 subprocess，用 NDJSON 协议通信。CI / 上游 agent 可以把 Amp 当"函数"调用。

两者对应的场景：
- DTW = 长期运行的云端 agent（几小时的编译测试）
- Stream-JSON = 短期 local subprocess（CI pipeline 里一步）
