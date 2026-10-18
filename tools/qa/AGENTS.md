# Manual native acceptance
> 只按显式参数打开并回收本次自建 demo 程序。

## 地位
手工 macOS 窗口/诊断探针，不替代静态检查、无窗口测试或实机视觉验收。
## 逻辑
native_window.py 要求 --run，使用根 cmd 构建独立临时程序，明确区分实际回调、当前 Build、PNG、compositor 和 AX 证据。
## 约束
不连接模型账户、不发现凭据、不操纵用户既有窗口；仅回收本次自建进程，清理放在 finally。普通测试/CI 不隐式运行。
## 业务域清单
| 路径 | 职责 |
|---|---|
| native_window.py | --run / --app direct或hub；可选 --launchservices，仅对自建临时 bundle 签名并启动 |
