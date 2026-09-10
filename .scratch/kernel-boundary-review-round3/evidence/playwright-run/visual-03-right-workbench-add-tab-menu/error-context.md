# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: visual.spec.ts >> 03 right workbench add-tab menu
- Location: tests/visual.spec.ts:49:1

# Error details

```
Error: expect(page).toHaveScreenshot(expected) failed

  411 pixels (ratio 0.01 of all image pixels) are different.

  Snapshot: 03-right-workbench-add-tab.png

Call log:
  - Expect "toHaveScreenshot(03-right-workbench-add-tab.png)" with timeout 5000ms
    - verifying given screenshot expectation
  - taking page screenshot
    - disabled all CSS animations
  - waiting for fonts to load...
  - fonts loaded
  - 411 pixels (ratio 0.01 of all image pixels) are different.
  - waiting 100ms before taking screenshot
  - taking page screenshot
    - disabled all CSS animations
  - waiting for fonts to load...
  - fonts loaded
  - captured a stable screenshot
  - 411 pixels (ratio 0.01 of all image pixels) are different.

```

# Page snapshot

```yaml
- generic:
  - generic:
    - generic:
      - complementary:
        - generic:
          - generic: A
          - generic:
            - heading [level=1]: ALVA Hub
            - paragraph: 一个项目，协同所有引擎。
        - navigation:
          - button:
            - generic:
              - strong: 新建会话
          - button:
            - generic:
              - strong: 自动化
          - button:
            - generic:
              - strong: 插件
          - button:
            - generic:
              - strong: 搜索
          - button:
            - generic:
              - strong: 设置
        - generic:
          - textbox:
            - /placeholder: 搜索会话…
        - generic:
          - generic:
            - generic: 会话
            - generic: "3"
          - generic:
            - generic:
              - button:
                - generic:
                  - strong: 重构交接数据的脱敏过滤器
                  - generic: alva-agent
                  - generic: claude · 已停放 · 2024-08-30
              - button:
                - generic:
                  - strong: 添加 Worker 队列状态接口
                  - generic: other-repo
                  - generic: codex · 进行中 · 2024-08-30
              - button:
                - generic:
                  - strong: 验证资源声明冲突规则的原型
                  - generic: scratch
                  - generic: alva · 已完成 · 2024-08-30
      - main:
        - generic:
          - generic:
            - generic:
              - strong: 将脱敏测试迁移到新表结构
              - generic: alva-agent
            - generic:
              - button:
                - generic: 详情
              - generic: 已完成
          - generic:
            - generic:
              - generic:
                - img "Agent 执行时间线概览":
                  - generic:
                    - strong: 执行时间线
                  - generic:
                    - generic: "0"
                    - generic: 140s
                    - generic: 280s
                  - generic:
                    - generic: 输入
                    - generic:
                      - generic "alva · 重构交接数据的脱敏过滤器 · +0ms"
                      - generic "claude · 将脱敏测试迁移到新表结构 · +200s"
                  - generic:
                    - generic: Agent
                    - generic:
                      - generic "alva · 已将 40 多种敏感信息形态合并为一次表驱动处理。 · +100s"
                      - generic "claude · 供应商触发速率限制：HTTP 429 · +200s"
                      - generic "claude · alva → claude 供应商触发速率限制：HTTP 429 · +220s"
                  - generic:
                    - generic: 工具
                    - generic:
                      - generic "alva · file_edit 已完成 · +100s"
                      - generic "claude · bash 已完成 · +279s"
                - article:
                  - generic:
                    - generic: 你
                    - generic: 用户消息
                  - paragraph: 重构交接数据的脱敏过滤器
                - article:
                  - generic:
                    - generic: alva
                    - generic: 操作已完成
                  - paragraph: file_edit 已完成
                  - button "file_edit src/redact.rs 210ms":
                    - generic:
                      - generic:
                        - strong: file_edit
                        - code: src/redact.rs
                      - generic: 210ms
                - article:
                  - generic:
                    - generic: alva
                    - generic: 助手消息
                  - paragraph: 已将 40 多种敏感信息形态合并为一次表驱动处理。
                - article:
                  - generic:
                    - generic: 你
                    - generic: 用户消息
                  - paragraph: 将脱敏测试迁移到新表结构
                - article:
                  - generic:
                    - generic: claude
                    - generic: 失败
                  - paragraph: 供应商触发速率限制：HTTP 429
                - article:
                  - generic:
                    - generic: claude
                    - generic: 自动回退
                  - paragraph: alva → claude 供应商触发速率限制：HTTP 429
                  - generic:
                    - button "停止" [disabled]
                    - button "切回 alva"
                    - button "查看交接信息"
                - article:
                  - generic:
                    - generic: claude
                    - generic: 操作已完成
                  - paragraph: bash 已完成
                  - button "bash 900ms":
                    - generic:
                      - generic:
                        - strong: bash
                      - generic: 900ms
            - generic:
              - status:
                - generic:
                  - strong: 允许 bash？
                  - paragraph: 2 个 Agent 需要访问 src/queue.rs。
                  - generic: worker-demo-1 · worker-demo-2
                - generic:
                  - button:
                    - generic: 允许
                  - button:
                    - generic: 拒绝
              - generic:
                - generic:
                  - textbox:
                    - /placeholder: 让 claude 检查、修改或解释这个项目…
                  - generic:
                    - generic:
                      - button
                      - combobox [disabled]:
                        - generic: 允许写入工作区
                      - combobox [disabled]
                      - generic:
                        - checkbox
                        - checkbox
                        - generic:
                          - strong: 共享对话记录
                    - generic:
                      - button:
                        - generic: 选择模型
                      - button [disabled]
                - generic:
                  - generic: 项目 · alva-agent
                  - generic: ·请选择可用模型
                  - generic: ·允许写入工作区 · 3 个回退引擎
      - generic:
        - separator
        - complementary:
          - generic:
            - generic:
              - tablist:
                - generic:
                  - generic:
                    - tab [selected]:
                      - generic: 检查器
                    - button
                - generic:
                  - button [expanded]
                  - button
                  - button
              - tabpanel:
                - generic:
                  - generic:
                    - region:
                      - generic:
                        - generic:
                          - button [disabled]:
                            - generic: 返回
                          - generic: native-a
                        - button:
                          - generic: 刷新
                      - generic:
                        - generic:
                          - generic:
                            - generic:
                              - strong: ALVA · 原生 Agent 检查器
                              - generic: native-alva-1 · 分支 branch-d
                            - generic:
                              - generic: 3 条记录
                              - generic: 历史
                          - generic:
                            - generic:
                              - generic:
                                - generic: 总耗时
                                - strong: 842ms
                              - generic:
                                - generic: 输入 / 输出 token
                                - strong: 820/164
                              - generic:
                                - generic: 轮次
                                - strong: "1"
                              - generic:
                                - generic: 子 Agent
                                - strong: "1"
                        - generic:
                          - generic:
                            - generic:
                              - strong: 子 Agent
                            - generic: "1"
                          - generic:
                            - generic:
                              - generic:
                                - strong: 子 Agent demo-nat
                                - generic: 派生调用 tool-demo-spawn
                              - generic:
                                - button:
                                  - generic: 打开
                        - generic:
                          - generic:
                            - generic:
                              - strong: Middleware 调用
                            - generic: "1"
                          - generic:
                            - generic:
                              - generic:
                                - strong: security
                                - generic: on_agent_start · success
                              - generic: 2ms
                              - generic:
                                - button:
                                  - generic: 打开
                        - generic:
                          - generic:
                            - generic:
                              - strong: 轮次
                            - generic: "1"
                          - generic:
                            - generic:
                              - generic:
                                - button [expanded]:
                                  - generic:
                                    - strong: 轮次 1
                                    - generic: 820ms · tool_use
                                  - generic: "1"
                              - generic:
                                - generic:
                                  - generic:
                                    - strong: LLM 调用
                                    - generic: 820/164 输入 / 输出 token
                                  - generic: 612ms
                                  - generic:
                                    - button:
                                      - generic: 打开
                                - generic:
                                  - generic:
                                    - strong: read_file
                                    - generic: tool-demo-1
                                  - generic: 104ms
                                  - generic:
                                    - button:
                                      - generic: 打开
                                - generic:
                                  - generic:
                                    - strong: security
                                    - generic: wrap_llm_call
                                  - generic: 614ms
                                  - generic:
                                    - button:
                                      - generic: 打开
                        - generic:
                          - generic:
                            - button:
                              - generic:
                                - strong: 原始持久化记录
                              - generic: "3"
                    - generic:
                      - generic:
                        - generic:
                          - strong: 检查器
                          - generic: 执行记录按轮次与尝试分组。Agent 关系位于 Agent 标签页。
                        - generic: 2 个轮次
                      - generic:
                        - generic:
                          - generic:
                            - button:
                              - generic:
                                - strong: 重构交接数据的脱敏过滤器
                                - generic: 已完成 · 1 次尝试
                        - generic:
                          - generic:
                            - button:
                              - generic:
                                - strong: 将脱敏测试迁移到新表结构
                                - generic: 失败 · 2 次尝试
                    - generic:
                      - generic:
                        - generic:
                          - strong: Handoff envelope
                          - generic: "Full cross-engine envelope: transcript, memory, skills, MCP."
                        - generic: "1"
                      - generic:
                        - generic:
                          - generic:
                            - button:
                              - generic:
                                - strong: full · alva → claude
                                - generic: 供应商触发速率限制：HTTP 429
                    - generic:
                      - generic:
                        - generic:
                          - strong: Branches
                          - generic: Engine branches with resource/worktree identity.
                        - generic: "2"
                      - generic:
                        - generic:
                          - generic:
                            - strong: alva/workspace-config · parked
                            - generic: workspace-config · b-alva
                          - generic: session native-a
                        - generic:
                          - generic:
                            - strong: claude/claude-sonnet-4-5 · active
                            - generic: claude-cli · b-claude
                          - generic: session native-c
                    - generic:
                      - generic:
                        - generic:
                          - strong: Timeline (permission / fallback lifecycle)
                        - generic: "1"
                      - generic:
                        - generic:
                          - generic:
                            - strong: engine_fallback · alva → claude
                            - generic: 供应商触发速率限制：HTTP 429
                          - generic: 2:27:00 PM
                    - generic:
                      - generic:
                        - generic:
                          - strong: Session memory
                        - generic: "1"
                      - generic:
                        - generic:
                          - generic:
                            - strong: 完成前始终运行 cargo test
                            - generic: mem-0
                    - generic:
                      - generic:
                        - generic:
                          - strong: 实时事件摘要
                        - generic: "7"
                      - generic:
                        - generic:
                          - generic:
                            - strong: alva · 用户消息
                            - generic: 重构交接数据的脱敏过滤器
                        - generic:
                          - generic:
                            - strong: alva · 操作已完成
                            - generic: file_edit 已完成
                        - generic:
                          - generic:
                            - strong: alva · 助手消息
                            - generic: 已将 40 多种敏感信息形态合并为一次表驱动处理。
                        - generic:
                          - generic:
                            - strong: claude · 用户消息
                            - generic: 将脱敏测试迁移到新表结构
                        - generic:
                          - generic:
                            - strong: claude · 失败
                            - generic: 供应商触发速率限制：HTTP 429
                        - generic:
                          - generic:
                            - strong: claude · 自动回退
                            - generic: alva → claude 供应商触发速率限制：HTTP 429
                        - generic:
                          - generic:
                            - strong: claude · 操作已完成
                            - generic: bash 已完成
  - menu "添加工作台标签页" [active] [ref=e1]:
    - generic [ref=e2]: 添加标签页
    - menuitem "文件" [ref=e3]
    - menuitem "审查" [ref=e9]
    - menuitem "Agent" [ref=e14]
    - menuitem "检查器 已打开" [disabled] [ref=e18]:
      - text: 检查器
      - generic [ref=e24]: 已打开
    - menuitem "任务" [ref=e25]
    - menuitem "用量" [ref=e29]
    - menuitem "浏览器" [ref=e33]
    - menuitem "文本" [ref=e39]
    - menuitem "Git" [ref=e45]
    - menuitem "终端" [ref=e50]
    - menuitem "引擎" [ref=e54]
    - menuitem "回退" [ref=e66]
    - menuitem "团队" [ref=e69]
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
  15  |   await expect(page).toHaveScreenshot("01-chat-shell.png", { fullPage: true });
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
> 53  |   await expect(page).toHaveScreenshot("03-right-workbench-add-tab.png", { fullPage: true });
      |                      ^ Error: expect(page).toHaveScreenshot(expected) failed
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