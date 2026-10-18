# Shared chat presentation
> UI 线程内的聊天状态与 MyGo 控件，不拥有运行、会话或订阅。

## 地位
由 direct 与 Hub 页面共同使用；框架文件受 desktop build tag 保护。Inspector 分类只读 native metadata，不创建存储 owner。
## 逻辑
Model 按实际准入、观察与终态更新；默认保留 64 行，direct 耐久页面显式选择 128 行。AttachRun 只重绑已存在运行的观察高水位，不生成消息或清空草稿。NativeView.Build 包含共享状态/错误与 Jump to latest，BuildChat 供产品自行布局状态栏。
## 约束
不依赖 direct/Hub/harness/host/engine，不读取凭据或自动调用模型。Stop 只调用所属产品提供的回调；显示停止需等待真实终态。列表虚拟化并保留阅读锚点，字段与事件只在 UI 线程访问。产品诊断字段留在产品 wrapper，不导出可变内部 ListState 或序号。
## 业务域清单
| 文件 | 职责 |
|---|---|
| model.go / model_test.go | 草稿、稳定消息、有界保留、观察重绑与迟到结果拒绝 |
| view_desktop.go / view_desktop_test.go | MyGo 输入/IME、显式 Send/Stop、Markdown、虚拟列表与阅读锚点 |
| inspection.go | 当前 metadata 页的 LLM/Tools/Middleware/Other 分类，不改变查询或事实 |
