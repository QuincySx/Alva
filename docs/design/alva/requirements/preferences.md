# preferences

> 来源：旧 JSON 包的 `requirements/preferences.json`（该包已于 2026-11-09 删除，见 git 提交 690c4525）。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。

## req_alva_preference_choices — 设置偏好选择行与共用选项菜单

状态：2026-10-26 当前版本仅显示颜色主题、14/16px正文大小和减弱动效。语言/密度暂固定并隐藏入口，保留既有保存值和共享组件，后续有真实能力再展示。
选择行复用Foundation Alva SettingRow.Control=Select，文字/图标垂直居中、对称padding；选项是数据，不为每个值建变体或新Token。颜色主题跟随系统/浅色/深色，字号从唯一共享PreferenceChoices读取。
选择只编辑本窗草稿，明确Save经同一PreferencesService/SQLite CAS原子保存；成功回执后跨窗生效，Reload读取真实已提交值。冲突/取消/读取失败保留草稿，不写默认库或提前改变主题。
验收：无会话即可打开；编辑不影响其他窗口；保存后实际颜色/字号同步，重开仍保留；失效revision拒绝；隐藏的语言/密度不占焦点且不篡改已存值。

绑定位置：
- `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Theme`
- `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Density`
- `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Language`
- `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / FontSize`

标注：
- [behavior-contract] req_alva_preference_choices — SettingRow Select当前值入口；点击/Enter/Space打开共用Popover+SelectOption；Esc返回焦点。值与选项为数据，不复制弹层画板，静态不保存。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Theme`
- [behavior-contract] req_alva_preference_choices — SettingRow Select当前值入口；点击/Enter/Space打开共用Popover+SelectOption；Esc返回焦点。值与选项为数据，不复制弹层画板，静态不保存。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Density`
- [behavior-contract] req_alva_preference_choices — SettingRow Select当前值入口；点击/Enter/Space打开共用Popover+SelectOption；Esc返回焦点。值与选项为数据，不复制弹层画板，静态不保存。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Language`
- [behavior-contract] req_alva_preference_choices — SettingRow Select当前值入口；点击/Enter/Space打开共用Popover+SelectOption；Esc返回焦点。值与选项为数据，不复制弹层画板，静态不保存。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / FontSize`
- [behavior-contract] req_alva_preference_choices — 本窗草稿由明确保存提交；重新加载读取已提交事实。共用Button的禁用/加载/焦点，实际CAS与回执由PreferencesService负责。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / PreferencesActions / SavePreferences`
- [behavior-contract] req_alva_preference_choices — 本窗草稿由明确保存提交；重新加载读取已提交事实。共用Button的禁用/加载/焦点，实际CAS与回执由PreferencesService负责。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / PreferencesActions / ReloadPreferences`

## req_alva_preference_motion — 开关外观、焦点与减弱动效

状态：2026-10-26 减弱动效为真实应用偏好，复用同一Save/Reload和CAS；默认关闭应用覆盖，即跟随系统要求，开启后额外减少应用动画。系统已要求减弱时不能由应用开关强制恢复动效。
行/开关复用Foundation SettingRow.Control=Switch，标签与控件居中、对称padding，使用既有焦点/disabled样式。编辑开关未Save不改变运行外观；Save提交后跨窗框架动画与共享paint-only Loading同时消费“系统要求 OR 应用已提交开关”。
减弱时保留静态忙碌图标和可读状态，不再转圈或持续请求动画帧；仍防重复操作，不用动画承担唯一状态提示。不为开关值、场景或每个Loading额外画状态大全。
验收：开启并Save后真实Loader停止请求帧，关闭覆盖后恢复系统行为；另一窗口和重启读到同一已提交值；冲突/失败保留编辑，系统减弱始终有效。

绑定位置：
- `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Motion`

标注：
- [behavior-contract] req_alva_preference_motion — 共用Switch，On为主题重点色；键盘焦点与选中值独立，真实动效/保存由产品实现。 — `pages/设置/31 通用.html` → `SettingsRegion / SettingsMain / Appearance / PreferenceRows / Motion`
