# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: visual.spec.ts >> 03 right workbench
- Location: tests/visual.spec.ts:24:1

# Error details

```
Error: expect(page).toHaveScreenshot(expected) failed

  411 pixels (ratio 0.01 of all image pixels) are different.

  Snapshot: 03-right-workbench.png

Call log:
  - Expect "toHaveScreenshot(03-right-workbench.png)" with timeout 5000ms
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
          - button "详情" [ref=e77] [cursor=pointer]
          - generic [ref=e79]: 已完成
      - generic [ref=e81]:
        - generic [ref=e83]:
          - img "Agent 执行时间线概览" [ref=e84]:
            - strong [ref=e86]: 执行时间线
            - generic [ref=e87]:
              - generic [ref=e88]: "0"
              - generic [ref=e89]: 140s
              - generic [ref=e90]: 280s
            - generic [ref=e91]:
              - generic [ref=e92]: 输入
              - generic [ref=e93]:
                - generic "alva · 重构交接数据的脱敏过滤器 · +0ms" [ref=e94]
                - generic "claude · 将脱敏测试迁移到新表结构 · +200s" [ref=e95]
            - generic [ref=e96]:
              - generic [ref=e97]: Agent
              - generic [ref=e98]:
                - generic "alva · 已将 40 多种敏感信息形态合并为一次表驱动处理。 · +100s" [ref=e99]
                - generic "claude · 供应商触发速率限制：HTTP 429 · +200s" [ref=e100]
                - generic "claude · alva → claude 供应商触发速率限制：HTTP 429 · +220s" [ref=e101]
            - generic [ref=e102]:
              - generic [ref=e103]: 工具
              - generic [ref=e104]:
                - generic "alva · file_edit 已完成 · +100s" [ref=e105]
                - generic "claude · bash 已完成 · +279s" [ref=e106]
          - article [ref=e107]:
            - generic [ref=e108]:
              - generic [ref=e109]: 你
              - generic [ref=e113]: 用户消息
            - paragraph [ref=e114]: 重构交接数据的脱敏过滤器
          - article [ref=e115]:
            - generic [ref=e116]:
              - generic [ref=e117]: alva
              - generic [ref=e121]: 操作已完成
            - paragraph [ref=e122]: file_edit 已完成
            - button "file_edit src/redact.rs 210ms" [ref=e123] [cursor=pointer]:
              - generic [ref=e124]:
                - generic [ref=e125]:
                  - strong [ref=e130]: file_edit
                  - code [ref=e131]: src/redact.rs
                - generic [ref=e132]: 210ms
          - article [ref=e133]:
            - generic [ref=e134]:
              - generic [ref=e135]: alva
              - generic [ref=e139]: 助手消息
            - paragraph [ref=e140]: 已将 40 多种敏感信息形态合并为一次表驱动处理。
          - article [ref=e141]:
            - generic [ref=e142]:
              - generic [ref=e143]: 你
              - generic [ref=e147]: 用户消息
            - paragraph [ref=e148]: 将脱敏测试迁移到新表结构
          - article [ref=e149]:
            - generic [ref=e150]:
              - generic [ref=e151]: claude
              - generic [ref=e155]: 失败
            - paragraph [ref=e156]: 供应商触发速率限制：HTTP 429
          - article [ref=e157]:
            - generic [ref=e158]:
              - generic [ref=e159]: claude
              - generic [ref=e163]: 自动回退
            - paragraph [ref=e164]: alva → claude 供应商触发速率限制：HTTP 429
            - generic [ref=e165]:
              - button "停止" [disabled] [ref=e166]
              - button "切回 alva" [ref=e168] [cursor=pointer]
              - button "查看交接信息" [ref=e170] [cursor=pointer]
          - article [ref=e172]:
            - generic [ref=e173]:
              - generic [ref=e174]: claude
              - generic [ref=e178]: 操作已完成
            - paragraph [ref=e179]: bash 已完成
            - button "bash 900ms" [ref=e180] [cursor=pointer]:
              - generic [ref=e181]:
                - strong [ref=e187]: bash
                - generic [ref=e188]: 900ms
        - generic [ref=e189]:
          - status [ref=e190]:
            - generic [ref=e191]:
              - strong [ref=e192]: 允许 bash？
              - paragraph [ref=e193]: 2 个 Agent 需要访问 src/queue.rs。
              - generic [ref=e194]: worker-demo-1 · worker-demo-2
            - generic [ref=e195]:
              - button "允许 2 个 bash 请求" [ref=e196] [cursor=pointer]:
                - generic [ref=e197]: 允许
              - button "拒绝 2 个 bash 请求" [ref=e198] [cursor=pointer]:
                - generic [ref=e199]: 拒绝
          - generic [ref=e200]:
            - generic [ref=e201]:
              - textbox "消息" [ref=e202]:
                - /placeholder: 让 claude 检查、修改或解释这个项目…
              - generic [ref=e203]:
                - generic [ref=e204]:
                  - button "添加上下文" [ref=e205] [cursor=pointer]
                  - combobox "权限配置" [disabled] [ref=e208]:
                    - generic: 允许写入工作区
                  - combobox [disabled] [ref=e211]
                  - generic [ref=e212] [cursor=pointer]:
                    - checkbox "共享对话记录" [ref=e213]
                    - checkbox
                    - strong [ref=e215]: 共享对话记录
                - generic [ref=e216]:
                  - button "模型与引擎" [ref=e217] [cursor=pointer]:
                    - generic [ref=e221]: 选择模型
                  - button "使用 claude 运行" [disabled] [ref=e222]
            - generic [ref=e226]:
              - generic [ref=e227]: 项目 · alva-agent
              - generic [ref=e228]: ·请选择可用模型
              - generic [ref=e229]: ·允许写入工作区 · 3 个回退引擎
  - generic [ref=e230]:
    - separator "调整工作台宽度" [ref=e231]
    - complementary [ref=e232]:
      - generic [ref=e234]:
        - tablist "工作台面板" [ref=e235]:
          - generic [ref=e237]:
            - tab "检查器" [selected] [ref=e238] [cursor=pointer]
            - button "关闭工作台 · 检查器" [ref=e245] [cursor=pointer]
          - generic [ref=e249]:
            - button "添加工作台标签页" [ref=e250] [cursor=pointer]
            - button "在独立窗口打开" [ref=e253] [cursor=pointer]
            - button "关闭工作台" [ref=e259] [cursor=pointer]
        - tabpanel "检查器" [ref=e263]:
          - generic [ref=e265]:
            - region "原生 Agent 检查器" [ref=e266]:
              - generic [ref=e267]:
                - generic [ref=e268]:
                  - button "返回" [disabled] [ref=e269]
                  - generic [ref=e273]: native-a
                - button "刷新" [ref=e274] [cursor=pointer]
              - generic [ref=e281]:
                - generic [ref=e282]:
                  - generic [ref=e283]:
                    - generic [ref=e284]:
                      - strong [ref=e285]: ALVA · 原生 Agent 检查器
                      - generic [ref=e286]: native-alva-1 · 分支 branch-d
                    - generic [ref=e287]:
                      - generic [ref=e288]: 3 条记录
                      - generic [ref=e289]: 历史
                  - generic [ref=e291]:
                    - generic [ref=e292]:
                      - generic [ref=e293]: 总耗时
                      - strong [ref=e294]: 842ms
                    - generic [ref=e295]:
                      - generic [ref=e296]: 输入 / 输出 token
                      - strong [ref=e297]: 820/164
                    - generic [ref=e298]:
                      - generic [ref=e299]: 轮次
                      - strong [ref=e300]: "1"
                    - generic [ref=e301]:
                      - generic [ref=e302]: 子 Agent
                      - strong [ref=e303]: "1"
                - generic [ref=e304]:
                  - generic [ref=e305]:
                    - strong [ref=e307]: 子 Agent
                    - generic [ref=e308]: "1"
                  - generic [ref=e311]:
                    - generic [ref=e316]:
                      - strong [ref=e317]: 子 Agent demo-nat
                      - generic [ref=e318]: 派生调用 tool-demo-spawn
                    - button "打开" [ref=e320] [cursor=pointer]
                - generic [ref=e322]:
                  - generic [ref=e323]:
                    - strong [ref=e325]: Middleware 调用
                    - generic [ref=e326]: "1"
                  - generic [ref=e329]:
                    - generic [ref=e330]:
                      - strong [ref=e331]: security
                      - generic [ref=e332]: on_agent_start · success
                    - generic [ref=e333]: 2ms
                    - button "打开" [ref=e335] [cursor=pointer]
                - generic [ref=e337]:
                  - generic [ref=e338]:
                    - strong [ref=e340]: 轮次
                    - generic [ref=e341]: "1"
                  - generic [ref=e344]:
                    - button "轮次 1 820ms · tool_use 1" [expanded] [ref=e346] [cursor=pointer]:
                      - generic [ref=e349]:
                        - strong [ref=e350]: 轮次 1
                        - generic [ref=e351]: 820ms · tool_use
                      - generic [ref=e352]: "1"
                    - generic [ref=e353]:
                      - generic [ref=e354]:
                        - generic [ref=e368]:
                          - strong [ref=e369]: LLM 调用
                          - generic [ref=e370]: 820/164 输入 / 输出 token
                        - generic [ref=e371]: 612ms
                        - button "打开" [ref=e373] [cursor=pointer]
                      - generic [ref=e375]:
                        - generic [ref=e379]:
                          - strong [ref=e380]: read_file
                          - generic [ref=e381]: tool-demo-1
                        - generic [ref=e382]: 104ms
                        - button "打开" [ref=e384] [cursor=pointer]
                      - generic [ref=e386]:
                        - generic [ref=e391]:
                          - strong [ref=e392]: security
                          - generic [ref=e393]: wrap_llm_call
                        - generic [ref=e394]: 614ms
                        - button "打开" [ref=e396] [cursor=pointer]
                - button "原始持久化记录 3" [ref=e400] [cursor=pointer]:
                  - strong [ref=e404]: 原始持久化记录
                  - generic [ref=e405]: "3"
            - generic [ref=e406]:
              - generic [ref=e407]:
                - generic [ref=e408]:
                  - strong [ref=e409]: 检查器
                  - generic [ref=e410]: 执行记录按轮次与尝试分组。Agent 关系位于 Agent 标签页。
                - generic [ref=e411]: 2 个轮次
              - generic [ref=e413]:
                - button "重构交接数据的脱敏过滤器 已完成 · 1 次尝试" [ref=e416] [cursor=pointer]:
                  - generic [ref=e419]:
                    - strong [ref=e420]: 重构交接数据的脱敏过滤器
                    - generic [ref=e421]: 已完成 · 1 次尝试
                - button "将脱敏测试迁移到新表结构 失败 · 2 次尝试" [ref=e424] [cursor=pointer]:
                  - generic [ref=e427]:
                    - strong [ref=e428]: 将脱敏测试迁移到新表结构
                    - generic [ref=e429]: 失败 · 2 次尝试
            - generic [ref=e430]:
              - generic [ref=e431]:
                - generic [ref=e432]:
                  - strong [ref=e433]: Handoff envelope
                  - generic [ref=e434]: "Full cross-engine envelope: transcript, memory, skills, MCP."
                - generic [ref=e435]: "1"
              - button "full · alva → claude 供应商触发速率限制：HTTP 429" [ref=e440] [cursor=pointer]:
                - generic [ref=e443]:
                  - strong [ref=e444]: full · alva → claude
                  - generic [ref=e445]: 供应商触发速率限制：HTTP 429
            - generic [ref=e446]:
              - generic [ref=e447]:
                - generic [ref=e448]:
                  - strong [ref=e449]: Branches
                  - generic [ref=e450]: Engine branches with resource/worktree identity.
                - generic [ref=e451]: "2"
              - generic [ref=e453]:
                - generic [ref=e454]:
                  - generic [ref=e455]:
                    - strong [ref=e456]: alva/workspace-config · parked
                    - generic [ref=e457]: workspace-config · b-alva
                  - generic [ref=e458]: session native-a
                - generic [ref=e459]:
                  - generic [ref=e460]:
                    - strong [ref=e461]: claude/claude-sonnet-4-5 · active
                    - generic [ref=e462]: claude-cli · b-claude
                  - generic [ref=e463]: session native-c
            - generic [ref=e464]:
              - generic [ref=e465]:
                - strong [ref=e467]: Timeline (permission / fallback lifecycle)
                - generic [ref=e468]: "1"
              - generic [ref=e471]:
                - generic [ref=e472]:
                  - strong [ref=e473]: engine_fallback · alva → claude
                  - generic [ref=e474]: 供应商触发速率限制：HTTP 429
                - generic [ref=e475]: 2:27:00 PM
            - generic [ref=e476]:
              - generic [ref=e477]:
                - strong [ref=e479]: Session memory
                - generic [ref=e480]: "1"
              - generic [ref=e484]:
                - strong [ref=e485]: 完成前始终运行 cargo test
                - generic [ref=e486]: mem-0
            - generic [ref=e487]:
              - generic [ref=e488]:
                - strong [ref=e490]: 实时事件摘要
                - generic [ref=e491]: "7"
              - generic [ref=e493]:
                - generic [ref=e495]:
                  - strong [ref=e496]: alva · 用户消息
                  - generic [ref=e497]: 重构交接数据的脱敏过滤器
                - generic [ref=e499]:
                  - strong [ref=e500]: alva · 操作已完成
                  - generic [ref=e501]: file_edit 已完成
                - generic [ref=e503]:
                  - strong [ref=e504]: alva · 助手消息
                  - generic [ref=e505]: 已将 40 多种敏感信息形态合并为一次表驱动处理。
                - generic [ref=e507]:
                  - strong [ref=e508]: claude · 用户消息
                  - generic [ref=e509]: 将脱敏测试迁移到新表结构
                - generic [ref=e511]:
                  - strong [ref=e512]: claude · 失败
                  - generic [ref=e513]: 供应商触发速率限制：HTTP 429
                - generic [ref=e515]:
                  - strong [ref=e516]: claude · 自动回退
                  - generic [ref=e517]: alva → claude 供应商触发速率限制：HTTP 429
                - generic [ref=e519]:
                  - strong [ref=e520]: claude · 操作已完成
                  - generic [ref=e521]: bash 已完成
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
> 27  |   await expect(page).toHaveScreenshot("03-right-workbench.png", { fullPage: true });
      |                      ^ Error: expect(page).toHaveScreenshot(expected) failed
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