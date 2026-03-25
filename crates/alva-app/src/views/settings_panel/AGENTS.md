# settings_panel
> 应用设置表单（LLM / 代理 / 主题）

## 地位
在 `views` 中作为设置 UI，被 `AgentPanel` 的 Settings 标签页内嵌使用。

## 逻辑
- `SettingsPanel` 从 `SettingsModel` 加载当前配置到本地 draft 字段。
- 用户点击字段进入编辑模式，键盘输入修改对应 draft。
- 点击 Save 时将 draft 组装为 `AppSettings` 并调用 `SettingsModel::update_settings` 持久化到 `~/.srow/settings.json`。
- 订阅 `SettingsModel` 事件以同步外部变更。
- Theme 切换通过三个按钮（System/Light/Dark），修改后标记 dirty。

## 约束
- 键盘输入同 InputBox，手动拼装字符，无 IME 支持。
- API Key 在非编辑态做掩码显示（首尾 4 字符 + 省略号）。
- Proxy URL 字段仅在 proxy enabled 时显示。
- 无表单校验逻辑（空值由 `has_api_key` 在发送时检查）。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| SettingsPanel | `settings_panel.rs` | 可聚焦设置表单，draft-then-save 语义，支持 LLM/Proxy/Theme 配置 |
| mod | `mod.rs` | 桶模块，re-export SettingsPanel |
