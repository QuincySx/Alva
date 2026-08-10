# Hub UI × AI 工具链调研

> 2026-08-09 调研。目标：确认"AI 直接输出/调试 UI"的现成工具，评估能否接到
> Hub 技术栈（Tauri 2 + React 18 + Vite 5 + TS + Tailwind 3 + zustand）上。

## 现有技术栈（Hub Desktop web 层）

| 层 | 技术 |
|---|---|
| 壳 | Tauri 2（wry webview = 系统 WebKit/WebView2，**不是 Chrome**） |
| 前端 | React 18 + Vite 5 + TypeScript + Tailwind 3 + zustand |
| 状态 | zustand + React state |
| 测试 | vitest + Testing Library（26 tests） |
| 已自研 | `web/scripts/wireframe.js` + `ui-wireframe.mjs`（CDP 线稿，零依赖） |

## 调研结论（GitHub 实测）

### 1. UI 生成（"直接输出 UI 界面"）
| 项目 | star | 栈匹配 | 说明 |
|---|---|---|---|
| **abi/screenshot-to-code** | 73.9k | ✅ React+Tailwind | 截图/设计稿/录屏 → 代码。架构 React/Vite + **FastAPI**，需 Gemini/GPT/Claude key（Ollama 质量差） |
| emilwallner/Screenshot-to-code | 16.5k | 部分 | 神经网络方案，较老 |

- 结论：**screenshot-to-code 方向正确、栈匹配，但引入成本高**（FastAPI 后端 + 多 key），不适合直接并入仓库。
- 可借鉴的方法：**视觉模型 + 截图 → 组件级评估/重构**（用户已有 anthropic key，可一次性地用，不落库）。

### 2. AI 调试 UI（"AI 的 debug"）
| 项目 | star | 适用性 |
|---|---|---|
| **microsoft/playwright-mcp** | 35.9k | 官方 MCP 服务器；**驱动 Chrome 浏览器**——可驱动 vite dev 页面做 AI 自动化 QA |
| browser-use/vibetest-use | 821 | Browser-Use 代理自动化 QA（Vibetest MCP） |
| browser-use-vision | 64 | SoM 视觉 grounding，辅助 vision 定位 |

- 关键限制：**Playwright 只能驱动真实浏览器**，Tauri 的 wry webview 不可被 Playwright 直接驱动。
  因此 AI 调试的落点是 **vite dev server 页面**（headless Chrome/CDP），与现有
  wireframe 工具同一路径——**生产 .app 内的 webview 无法用 Playwright 自动化**。

### 3. 已具备 + 建议的闭环（零新增重依赖）
1. **文本线稿**（已有）：`scripts/ui-wireframe.sh` → 布局树 + ASCII。
2. **截图**（已有）：headless Chrome `--screenshot` 同路径。
3. **交互驱动**（已有）：`--click` 模拟；CDP 可扩展为完整 DevTools 调试。
4. **AI 评估闭环（建议）**：
   - 方案 A（无 vision 模型）：线稿文本 + 组件清单 → 文本 LLM 做布局/可访问性/密度审查。
   - 方案 B（有 vision key）：截图 → vision 模型（用户 anthropic）做视觉审查，输出
     Tailwind 级修复建议，人工合并（或按 4.6 节规则授权 alva worker 执行）。
   - 方案 C（自动化 QA）：**按需**接入 playwright-mcp 驱动 dev server（评估后决定）。

## 建议的下一步（决策留给用户）
- [ ] A. 接受现状：wireframe + CDP 已是够用的"AI 可读 UI 界面"，继续迭代。
- [ ] B. 引入方案 B：一次性用 anthropic key 跑截图审查闭环，验证价值后固化脚本。
- [ ] C. 评估 playwright-mcp：为 dev server 加自动化 QA（重依赖，需 npm 安全 gate）。

## 本地模型方案（零成本，2026-08-09 落地）

`scripts/ui-review.sh`：线稿（文本）→ 任意 OpenAI 兼容模型 → 结构化 UI 审查。
不需要 vision（布局/密度/对齐/可访问性/一致性都从线稿可推断）。

```bash
ollama serve &                      # 本地 runtime（或任意 OpenAI 兼容端点）
./scripts/ui-review.sh --model=qwen3:8b --size=1180x760
# 端点可配：OLLAMA_URL=http://host:11434（默认 localhost）
```

注：本机当前未探测到 Ollama 服务/模型目录（`/api/tags` 无响应）。
模型就绪后该脚本直接可用。

## 实测记录（2026-08-09，端点 10.10.1.100:10443）

- **Qwen3.5-9B-MLX-4bit** ✅ 稳定。已产出 2 轮审查（默认态+展开态）：
  - 抓到真问题并修复：折叠按钮 20x20→28px、补 aria-expanded。
  - 展开态审查结论：布局健康；9B 有误判倾向（把 sidebar/timeline 混淆、
    误判 composer 未贴底）——审查需甄别，别盲信。
- **Qwen3.6-27B / 35B** ❌ 服务端内存不足（Metal 上限 21.3GB，27B 需 16.6GB+）。
- **Qwen3-VL-4B** ❌ 服务端 bug：生成时 `[concatenate] shapes (1,32000),(1,151936)`
  vocab 拼接错误，长输出必崩（短输出偶尔侥幸）。`ui-review-vision.sh` 已就绪，
  等服务端修好即用。
- **Qwen3.5-4B-DFlash** ❌ 加载失败（58 参数不匹配，模型文件问题）。

结论：文本闭环用 9B（稳定）；视觉闭环待 VL-4B 服务端修复。

## 模型迁移完成（2026-08-09）

主力审查模型：**Qwen3.5-9B-mlx-vlm-mxfp4**（RepublicOfKorokke，5.6GB）
- 验证：model-verify.sh 双探针通过（VISION 识别 6 面板 / TEXT 292px 正确）；
  完整视觉审查稳定生成。
- 替代 Qwen3.5-9B-MLX-4bit（mlx-community 格式，可能踩 omlx #2385 类型误判），
  一个模型覆盖文字+视觉审查。建议删除旧 9B 释放 ~6.5GB（27B 或可复活）。
- 规避清单（omlx issue 实证）：Qwen3-VL(#2374)、GLM-5.2(#2099/#2347)、
  MiniCPM-o-4.5(#2387)、DeepSeek-V4(#2469)、Mistral tekken(#1992)。

## 开发循环（AI 审查驱动，2026-08-10 固化）

每个 UI 改动走：**改 → wireframe 线稿复核（结构）→ 视觉审查（配色/密度）→ 修复 → 复验**。
实测此循环在 iter44-74 抓到并修复：topbar 被 composer-model 撑高、sidebar 5 面板溢出、
session-panel 被 flex 压缩到 h=0、折叠按钮 WCAG 尺寸、aria-expanded 缺失、对比度不足、
readiness 密度、占位符可读性、status-pill aria-live、最小窗口健康等 10+ 真实问题。
规则：AI 审查结论必须经量化/代码验证后才采纳（模型有误判倾向，如混淆 sidebar/timeline）。

## 模型质量门槛（实测）
- 9B 级：可用（能理解线稿布局语义，虽有误判但可甄别）。
- 2B 级：**不可用**——把 y 排序的并列面板误判为嵌套、把 flex 并列误判为覆盖。
  审查模型的模型下限约在 9B 档；`--steps` 工具本身已验证工作。

## 踩坑记录（2026-08-10）
- "视觉审查幻觉"（模型描述白色背景/错误页）根因是 **dev server 僵死**——截到的是
  浏览器错误页（Chrome ERR 文案），模型描述是准确的。审查前先 `curl -s localhost:1421` 确认。
- 文本审查（线稿）天然缺 aria/颜色/圆角信息——模型会把这些当"缺失"报。视觉问题用
  视觉审查（截图），结构问题用文本审查（线稿），分工明确。
- 模型"长输出卡死"可能是 oMLX 内存守卫中止（`process memory limit exceeded`）而非模型
  bug——先查服务器内存/任务占用，再怀疑模型。

## 审查循环收敛记录（iter150）
- 三维度正向：一致性(Consistent) / 布局(经典模式) / 可访问性(尺寸达标无缺口) 均获模型明确通过。
- 主观波动：同一 UI 偶现"割裂/拼凑"负面反馈——与正向审查矛盾；代码层 token（圆角两级/
  扁平/字体层级/单一 accent）统一，判定为模型主观波动，非 bug。甄别原则：**负反馈须与
  代码事实核对，与既有正向审查矛盾的优先信代码**。
- 修复沉淀：topbar 撑高 / sidebar 溢出 / session-panel h=0 / 折叠 WCAG+aria / 对比度 4.88 /
  readiness 密度 / 占位符 / status aria-live / failed 红调 / 徽章 12px+border / summary 换行 /
  footer 间距 / 面板 aria-labelledby / session 相对时间+结果 等 20+ 项。
