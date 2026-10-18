# L6 · ALVA Native Foundation Command
> 显式配置的 MyGo 自绘入口，默认只装配本地 demo，不自动执行任务。

## 地位

消费 harness、platform/host 与 apps/desktop，不经过 EngineRuntime、不让 SDK 依赖 UI；所有文件受 `desktop` build tag 控制。

## 逻辑

默认或 `--demo` 装配确定性本地模型。`--live --endpoint <full-url> --model <id>` 显式选真实服务；可额外 `--api-key-stdin` 从本次可取消pipe/regular文件标准输入有界读取凭据，不查环境或既有用户配置。只有点击 Send 发出请求。应用退出由 Controller 所有者请求取消并等待，UI 订阅释放不调用 Stop。
显式--data-dir启用独立apps/direct耐久会话首页/历史/附件引用/冷typed Inspector，--session只选已存在的32-hex native ID，绝不自动Send。--workspace仅绑定新会话，已保存会话恢复原workspace且校验route指纹；live逐实际call审批文件写入，不因目录默认授予Shell/MCP/network。四provider-kind共用canonical Build。无data-dir保持易失foundation而不隐式创建存储；两个模式都不进入Hub/EngineRuntime。
可选 --debug-listen 数字loopback，独立token仅经 --startup-secrets-stdin 精确JSON输入（与raw key stdin互斥）；prepare不监听，进入UI前显式Start。--debug-controls和--debug-execution分开授权。关闭先join诊断，再关闭Controller；diagnostic-only shutdown保留Agent可继续Send。

## 约束

无 npm、HTML、React、WebView、旧配置迁移和 Rust FFI；此入口不宣称已完成 Hub。不要把 API key 放到命令行、日志或错误文本。演示不启动 MCP/外部工具/网络，构建与测试不启动窗口。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Entry | `main.go` | 可取消的冷启动/显式路由、harness装配与runtime shutdown；进入窗口前还原启动signal handler |
| Startup regressions | `main_test.go` | 实际open pipe无EOF取消/不关闭借用stdin、拒绝超限key且零模型/窗口/Store、启动context结束后真正Send仍可执行 |
