# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: visual.spec.ts >> 01 chat shell
- Location: tests/visual.spec.ts:12:1

# Error details

```
Error: expect(page).toHaveScreenshot(expected) failed

  509 pixels (ratio 0.01 of all image pixels) are different.

  Snapshot: 01-chat-shell.png

Call log:
  - Expect "toHaveScreenshot(01-chat-shell.png)" with timeout 5000ms
    - verifying given screenshot expectation
  - taking page screenshot
    - disabled all CSS animations
  - waiting for fonts to load...
  - fonts loaded
  - 509 pixels (ratio 0.01 of all image pixels) are different.
  - waiting 100ms before taking screenshot
  - taking page screenshot
    - disabled all CSS animations
  - waiting for fonts to load...
  - fonts loaded
  - captured a stable screenshot
  - 509 pixels (ratio 0.01 of all image pixels) are different.

```

# Page snapshot

```yaml
- generic [ref=e3]:
  - complementary [ref=e4]:
    - generic [ref=e5]:
      - generic [ref=e6]: A
      - generic [ref=e7]:
        - heading "ALVA Hub" [level=1] [ref=e8]
        - paragraph [ref=e9]: 一个项目，协同所有引擎。
    - navigation "主导航" [ref=e10]:
      - button [ref=e11] [cursor=pointer]:
        - strong [ref=e16]: 新建会话
      - button [ref=e17] [cursor=pointer]:
        - strong [ref=e24]: 自动化
      - button [ref=e25] [cursor=pointer]:
        - strong [ref=e31]: 插件
      - button [ref=e32] [cursor=pointer]:
        - strong [ref=e38]: 搜索
      - button [ref=e39] [cursor=pointer]:
        - strong [ref=e45]: 设置
    - textbox "搜索会话" [ref=e47]:
      - /placeholder: 搜索会话…
    - generic [ref=e48]:
      - generic [ref=e49]:
        - generic [ref=e50]: 会话
        - generic [ref=e51]: "3"
      - generic [ref=e54]:
        - button "重构交接数据的脱敏过滤器 alva-agent claude · 已停放 · 2024-08-30" [ref=e55] [cursor=pointer]:
          - generic [ref=e56]:
            - strong [ref=e57]: 重构交接数据的脱敏过滤器
            - generic [ref=e58]: alva-agent
            - generic [ref=e59]: claude · 已停放 · 2024-08-30
        - button "添加 Worker 队列状态接口 other-repo codex · 进行中 · 2024-08-30" [ref=e60] [cursor=pointer]:
          - generic [ref=e61]:
            - strong [ref=e62]: 添加 Worker 队列状态接口
            - generic [ref=e63]: other-repo
            - generic [ref=e64]: codex · 进行中 · 2024-08-30
        - button "验证资源声明冲突规则的原型 scratch alva · 已完成 · 2024-08-30" [ref=e65] [cursor=pointer]:
          - generic [ref=e66]:
            - strong [ref=e67]: 验证资源声明冲突规则的原型
            - generic [ref=e68]: scratch
            - generic [ref=e69]: alva · 已完成 · 2024-08-30
  - main [ref=e70]:
    - generic [ref=e71]:
      - generic [ref=e72]:
        - generic [ref=e73]:
          - strong [ref=e74]: 将脱敏测试迁移到新表结构
          - generic [ref=e75]: alva-agent
        - generic [ref=e76]:
          - button "打开工作台" [ref=e77] [cursor=pointer]
          - button "详情" [ref=e81] [cursor=pointer]
          - generic [ref=e83]: 已完成
      - generic [ref=e85]:
        - generic [ref=e87]:
          - img "Agent 执行时间线概览" [ref=e88]:
            - strong [ref=e90]: 执行时间线
            - generic [ref=e91]:
              - generic [ref=e92]: "0"
              - generic [ref=e93]: 140s
              - generic [ref=e94]: 280s
            - generic [ref=e95]:
              - generic [ref=e96]: 输入
              - generic [ref=e97]:
                - generic "alva · 重构交接数据的脱敏过滤器 · +0ms" [ref=e98]
                - generic "claude · 将脱敏测试迁移到新表结构 · +200s" [ref=e99]
            - generic [ref=e100]:
              - generic [ref=e101]: Agent
              - generic [ref=e102]:
                - generic "alva · 已将 40 多种敏感信息形态合并为一次表驱动处理。 · +100s" [ref=e103]
                - generic "claude · 供应商触发速率限制：HTTP 429 · +200s" [ref=e104]
                - generic "claude · alva → claude 供应商触发速率限制：HTTP 429 · +220s" [ref=e105]
            - generic [ref=e106]:
              - generic [ref=e107]: 工具
              - generic [ref=e108]:
                - generic "alva · file_edit 已完成 · +100s" [ref=e109]
                - generic "claude · bash 已完成 · +279s" [ref=e110]
          - article [ref=e111]:
            - generic [ref=e112]:
              - generic [ref=e113]: 你
              - generic [ref=e117]: 用户消息
            - paragraph [ref=e118]: 重构交接数据的脱敏过滤器
          - article [ref=e119]:
            - generic [ref=e120]:
              - generic [ref=e121]: alva
              - generic [ref=e125]: 操作已完成
            - paragraph [ref=e126]: file_edit 已完成
            - button "file_edit src/redact.rs 210ms" [ref=e127] [cursor=pointer]:
              - generic [ref=e128]:
                - generic [ref=e129]:
                  - strong [ref=e134]: file_edit
                  - code [ref=e135]: src/redact.rs
                - generic [ref=e136]: 210ms
          - article [ref=e137]:
            - generic [ref=e138]:
              - generic [ref=e139]: alva
              - generic [ref=e143]: 助手消息
            - paragraph [ref=e144]: 已将 40 多种敏感信息形态合并为一次表驱动处理。
          - article [ref=e145]:
            - generic [ref=e146]:
              - generic [ref=e147]: 你
              - generic [ref=e151]: 用户消息
            - paragraph [ref=e152]: 将脱敏测试迁移到新表结构
          - article [ref=e153]:
            - generic [ref=e154]:
              - generic [ref=e155]: claude
              - generic [ref=e159]: 失败
            - paragraph [ref=e160]: 供应商触发速率限制：HTTP 429
          - article [ref=e161]:
            - generic [ref=e162]:
              - generic [ref=e163]: claude
              - generic [ref=e167]: 自动回退
            - paragraph [ref=e168]: alva → claude 供应商触发速率限制：HTTP 429
            - generic [ref=e169]:
              - button "停止" [disabled] [ref=e170]
              - button "切回 alva" [ref=e172] [cursor=pointer]
              - button "查看交接信息" [ref=e174] [cursor=pointer]
          - article [ref=e176]:
            - generic [ref=e177]:
              - generic [ref=e178]: claude
              - generic [ref=e182]: 操作已完成
            - paragraph [ref=e183]: bash 已完成
            - button "bash 900ms" [ref=e184] [cursor=pointer]:
              - generic [ref=e185]:
                - strong [ref=e191]: bash
                - generic [ref=e192]: 900ms
        - generic [ref=e193]:
          - status [ref=e194]:
            - generic [ref=e195]:
              - strong [ref=e196]: 允许 bash？
              - paragraph [ref=e197]: 2 个 Agent 需要访问 src/queue.rs。
              - generic [ref=e198]: worker-demo-1 · worker-demo-2
            - generic [ref=e199]:
              - button "允许 2 个 bash 请求" [ref=e200] [cursor=pointer]:
                - generic [ref=e201]: 允许
              - button "拒绝 2 个 bash 请求" [ref=e202] [cursor=pointer]:
                - generic [ref=e203]: 拒绝
          - generic [ref=e204]:
            - generic [ref=e205]:
              - textbox "消息" [ref=e206]:
                - /placeholder: 让 claude 检查、修改或解释这个项目…
              - generic [ref=e207]:
                - generic [ref=e208]:
                  - button "添加上下文" [ref=e209] [cursor=pointer]
                  - combobox "权限配置" [disabled] [ref=e212]:
                    - generic: 允许写入工作区
                  - combobox [disabled] [ref=e215]
                  - generic [ref=e216] [cursor=pointer]:
                    - checkbox "共享对话记录" [ref=e217]
                    - checkbox
                    - strong [ref=e219]: 共享对话记录
                - generic [ref=e220]:
                  - button "模型与引擎" [ref=e221] [cursor=pointer]:
                    - generic [ref=e225]: 选择模型
                  - button "使用 claude 运行" [disabled] [ref=e226]
            - generic [ref=e230]:
              - generic [ref=e231]: 项目 · alva-agent
              - generic [ref=e232]: ·请选择可用模型
              - generic [ref=e233]: ·允许写入工作区 · 3 个回退引擎
  - separator
  - complementary:
    - tablist:
      - generic:
        - tab [selected]:
          - generic: Agent
        - button
      - generic:
        - button
        - button
        - button
    - generic:
      - generic:
        - generic:
          - strong: Agent
          - generic: 显示子 Agent 关系，不显示执行调试记录。
          - generic: "2"
        - generic:
          - generic:
            - generic:
              - button [expanded]:
                - generic:
                  - strong: 主 Agent
                  - generic: ps-demo-2
            - generic:
              - generic:
                - generic:
                  - generic:
                    - button [expanded]:
                      - generic:
                        - strong: w-demo-1
                        - generic: codex · 进行中
                    - generic:
                      - button:
                        - generic: 打开
                  - generic:
                    - generic:
                      - generic:
                        - generic:
                          - strong: 实现队列状态接口
                          - generic: w-demo-1
                        - generic: 运行中
                - generic:
                  - generic:
                    - button [expanded]:
                      - generic:
                        - strong: w-demo-2
                        - generic: alva · 已休眠
                    - generic:
                      - button:
                        - generic: 打开
                  - generic:
                    - generic:
                      - generic:
                        - generic:
                          - strong: 为 Worker 接口编写 API 文档
                          - generic: w-demo-2
                        - generic: 排队中
```

# Test source

```ts
  1   | // INPUT:  Running Vite Hub, deterministic demo bridge, and Playwright
  2   | // OUTPUT: Aggregate hierarchy screenshots plus Inspector drill/back and model-picker coverage
  3   | // POS:    Visual contract connecting Cindy lineworks to the ALVA component system
  4   | 
  5   | import { expect, test, type Page } from "@playwright/test";
  6   | 
  7   | async function settle(page: Page) {
  8   |   await page.locator("body").waitFor({ state: "visible" });
  9   |   await page.evaluate(() => document.fonts.ready);
  10  | }
  11  | 
  12  | test("01 chat shell", async ({ page }) => {
  13  |   await page.goto("/?demo=1&session=ps-demo-1");
  14  |   await settle(page);
> 15  |   await expect(page).toHaveScreenshot("01-chat-shell.png", { fullPage: true });
      |                      ^ Error: expect(page).toHaveScreenshot(expected) failed
  16  | });
  17  | 
  18  | test("02 new conversation", async ({ page }) => {
  19  |   await page.goto("/?demo=1");
  20  |   await settle(page);
  21  |   await expect(page).toHaveScreenshot("02-new-conversation.png", { fullPage: true });
  22  | });
  23  | 
  24  | test("03 right workbench", async ({ page }) => {
  25  |   await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  26  |   await settle(page);
  27  |   await expect(page).toHaveScreenshot("03-right-workbench.png", { fullPage: true });
  28  | });
  29  | 
  30  | test("03 Inspector drills into Agent and LLM pages then backs out", async ({ page }) => {
  31  |   await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  32  |   await settle(page);
  33  |   const inspector = page.getByRole("region", { name: /Native Agent Inspector|原生 Agent 检查器/ });
  34  |   const child = inspector.locator(".ui-list-row").filter({ hasText: /Child Agent demo-nat|子 Agent demo-nat/ });
  35  |   await child.getByRole("button", { name: /Open|打开/ }).click();
  36  |   await expect(inspector).toContainText("native-a / demo-nat");
  37  | 
  38  |   const llm = inspector.locator(".ui-list-row").filter({ hasText: /LLM call|LLM 调用/ });
  39  |   await llm.getByRole("button", { name: /Open|打开/ }).click();
  40  |   await expect(inspector).toContainText(/Messages sent|发送给模型的消息/);
  41  |   await expect(page).toHaveScreenshot("03-inspector-llm-detail.png", { fullPage: true });
  42  | 
  43  |   await inspector.getByRole("button", { name: /Back|返回/ }).click();
  44  |   await expect(inspector).toContainText(/LLM call|LLM 调用/);
  45  |   await inspector.getByRole("button", { name: /Back|返回/ }).click();
  46  |   await expect(inspector).toContainText(/Child Agent demo-nat|子 Agent demo-nat/);
  47  | });
  48  | 
  49  | test("03 right workbench add-tab menu", async ({ page }) => {
  50  |   await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  51  |   await page.getByRole("button", { name: /Add workbench tab|添加工作台标签页/ }).click();
  52  |   await settle(page);
  53  |   await expect(page).toHaveScreenshot("03-right-workbench-add-tab.png", { fullPage: true });
  54  | });
  55  | 
  56  | test("03 right workbench manages dynamic tabs and width", async ({ page }) => {
  57  |   await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  58  |   await page.getByRole("button", { name: /Add workbench tab|添加工作台标签页/ }).click();
  59  |   await page.getByRole("menuitem", { name: /Terminal|终端/ }).click();
  60  |   await expect(page.getByRole("tab", { name: /Terminal|终端/ })).toHaveAttribute("aria-selected", "true");
  61  | 
  62  |   const resize = page.getByRole("separator", { name: /Resize workbench|调整工作台宽度/ });
  63  |   await resize.press("ArrowLeft");
  64  |   await expect(resize).toHaveAttribute("aria-valuenow", "464");
  65  | 
  66  |   await page.getByRole("button", { name: /Close workbench · Terminal|关闭工作台 · 终端/ }).click();
  67  |   await expect(page.getByRole("tab", { name: /Inspector|检查器/ })).toHaveAttribute("aria-selected", "true");
  68  |   await expect(page.getByRole("tab", { name: /Terminal|终端/ })).toHaveCount(0);
  69  | });
  70  | 
  71  | test("03 right workbench becomes a narrow overlay", async ({ page }) => {
  72  |   await page.setViewportSize({ width: 860, height: 900 });
  73  |   await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  74  |   await settle(page);
  75  |   await expect(page).toHaveScreenshot("03-right-workbench-narrow.png", { fullPage: true });
  76  | });
  77  | 
  78  | test("04 automation", async ({ page }) => {
  79  |   await page.goto("/?demo=1&page=automations");
  80  |   await page.getByRole("button", { name: /Nightly self-healing tests|夜间自修复测试/ }).click();
  81  |   await settle(page);
  82  |   await expect(page).toHaveScreenshot("04-automation.png", { fullPage: true });
  83  | });
  84  | 
  85  | test("05 plugins and skills", async ({ page }) => {
  86  |   await page.goto("/?demo=1&page=extensions");
  87  |   await settle(page);
  88  |   await expect(page).toHaveScreenshot("05-plugins-skills.png", { fullPage: true });
  89  | });
  90  | 
  91  | test("06 settings", async ({ page }) => {
  92  |   await page.goto("/?demo=1&page=settings");
  93  |   await settle(page);
  94  |   await expect(page).toHaveScreenshot("06-settings.png", { fullPage: true });
  95  | });
  96  | 
  97  | test("07 context menus", async ({ page }) => {
  98  |   await page.goto("/?demo=1");
  99  |   await page.getByRole("button", { name: /Refactor the handoff|重构交接/ }).click({ button: "right" });
  100 |   await settle(page);
  101 |   await expect(page).toHaveScreenshot("07-context-menus.png", { fullPage: true });
  102 | });
  103 | 
  104 | test("model picker stays anchored to composer", async ({ page }) => {
  105 |   await page.goto("/?demo=1&session=ps-demo-1");
  106 |   await page.getByRole("button", { name: /Model and engine|模型与引擎/ }).click();
  107 |   await settle(page);
  108 |   await expect(page).toHaveScreenshot("08-model-picker.png", { fullPage: true });
  109 | });
  110 | 
```