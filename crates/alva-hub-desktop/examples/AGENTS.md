# Hub Desktop examples
> 可重跑的产品边界验证夹具，不是生产启动入口。

## 地位
仅消费公开 L7 产品 seam；不改 SDK/kernel，不读取真实账号或运行真实 engine。

## 逻辑
`native_ui_fixture` 要求一个已存在且为空的绝对目录，在其中创建私有 Hub store、两个本地 Git worktree、耐久消息/工具正文与浏览器测试页。

## 约束
- 输入目录必须为空，不能覆盖已有数据；生成数据不自动删除。
- Git 身份命令级固定，清空继承环境，禁用户/系统配置和 hook；无远端/push。
- 配合 debug-only `--isolated-ui-data <fixture>/data`：临时主 WebView，不读取共享 Provider/config/auth、不运行 engine 或收费测试；不是真实账号验收。

## 业务域清单
| 文件 | 职责 |
|---|---|
| native_ui_fixture.rs | 确定性持久化和双工作树/浏览器夹具生成 |
