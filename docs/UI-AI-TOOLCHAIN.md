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
