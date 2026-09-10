# R22: 插件安装状态与注册操作必须来自真实数据

**Priority:** P2
**Axis:** Hub 实测 / 产品完整性
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** 无
**Status:** ready-for-agent

## 问题与证据

原生插件页无需读取安装注册表就显示 GitHub、TapTap Maker“已安装”；product-pages.tsx:240–249 将这些条目硬编码 installed:true。
“安装”只向局部 Set 加 id；“添加扩展→注册”在 :283–289 只清空输入并关闭弹窗，既不登记也不接入 agent 工具；“在对话中使用”仅导航到聊天。
这些状态出现在真实 Tauri 面，而非仅 demo。实测插件页截图/tree 在 evidence/native/native-plugins；注册/安装处理器为静态核对，未声称真的安装了包。

## 实现范围

生产目录只展示真实 registry/manifest 的安装与可用性；未实现注册/安装的入口禁用并说明，样例保留在 demo/UI Lab。若接入真实操作，结果必须由后端确认后才显示成功，失败保留输入。不要自行引入网络下载或新的插件系统。

## 验收

- [ ] 无实际安装记录时不显示已安装；切页和重启后的状态与 registry 一致。
- [ ] 注册可成功落盘并进入可用列表，或诚实显示尚不支持；不静默清空输入伪装完成。
- [ ] “在对话中使用”对应能力确实进入目标 agent，或明确它只是新会话导航。
- [ ] demo 样例与生产数据有可验证隔离。

## 执行记录

- Commit：待填写
- 实际验证命令及结果：待填写

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
