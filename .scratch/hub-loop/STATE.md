# 迭代日志


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint（iter38 末，压缩恢复点）
全量绿：hub-desktop 138 lib / hub-cli 25 / hub-core 21 / adapter-codex 9 / adapter-alva 12 / web 23 / ci-deps 46
真实引擎打包运行全覆盖：alva(4) codex(fresh+resume+fallback+worker) claude light(14) alva→codex handoff
修复清单：codex resume -C/-s；alva providers{} 解析；React key 含可编辑字段；clearAllMocks 不清实现(mock 泄漏)
剩余队列(iter39+)：watchdog stall 真实观察；cindy 演进(浏览器驱动/MCP 深化/automation 评估)；worker 中断真实引擎观察
恢复：读本文件续跑；每轮先 cargo test -p alva-hub-desktop --lib -p alva-hub-cli -p alva-hub-core + npm test(web/)
磁盘纪律：每轮 cargo clean；当前 48Gi 可用


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 2（iter43 末）
UI 反馈已回应：视觉系统重写(中性深色/单一 accent/去渐变/排版提升) + sidebar 可折叠
全量绿：hub-desktop 138 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 25 / ci 46
恢复指令：读本文件 → iter44 续；先 cargo test -p alva-hub-desktop --lib + npm test(web/)
剩余队列：watchdog stall 真实观察；cindy 演进深化；UI 主区 IA(三列/tab) 候选


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 3（iter51 末）
全量绿：hub-desktop 138 / hub-cli 25 / hub-core 21 / adapters 21 / web 26 / ci-deps 46
新增工具：web/scripts/wireframe.js + ui-wireframe.mjs + scripts/ui-wireframe.sh（UI 线稿，CDP 零依赖）
UI 反馈已闭环：视觉重写 + sidebar 折叠(默认收起 fallback/team) + 线稿发现修 2 个布局 bug
恢复指令：读本文件 → iter52 续；UI 改动后用 scripts/ui-wireframe.sh 线稿复核


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 4（iter71 末）
全量绿：hub-desktop 139 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 29 / ci 46
工具链完整：wireframe(线稿) + ui-review.sh(文本, 含 --steps) + ui-review-vision.sh(视觉, --click) + model-verify.sh + ?demo=1 数据态
模型：mxfp4 视觉可用(长输出不稳)；MLX-4bit 文本待恢复(SSD 清理后)；2B 兜底
模型相关待办：MLX-4bit 恢复后跑 ui-review.sh --steps 验证；mxfp4 稳定后审数据态
UI：重设计 + 折叠 + 对比度/可读性/密度/aria 系列修复(均由 AI 审查驱动)
恢复指令：读本文件 → iter72 续；先 npm test(web/) + cargo test -p alva-hub-desktop --lib


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 5（iter74 末）
全量绿：hub-desktop 139 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 29 / ci 46
本轮本地完成：bridge 100% 覆盖盘点、isolate 传递测试、最小窗口布局检查、ui-review --steps、demo 数据态、session-panel h=0 修复、sidebar 紧凑化
模型阻塞（外部依赖）：两个 9B 文本仍卡（服务器任务占用/omlx 状态），mxfp4 视觉不稳
恢复指令：读本文件 → iter75 续；模型恢复后优先跑 ui-review.sh --steps 验证 + 数据态视觉复审


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 6（iter82 末）
全绿：hub-desktop 139 / hub-cli 25 / hub-core 21 / adapters 21 / web 29 / ci 46
模型：mxfp4 文本+视觉全恢复（根因=内存守卫，非模型 bug）；最终配置 mxfp4+ASR
工具链全通：ui-review.sh --steps（文本）/ ui-review-vision.sh --steps --click（视觉）/ model-verify / ?demo=1 / wireframe
踩坑已固化：审查前 curl 验 dev server；文本审结构、视觉审配色；长输出卡=查内存守卫
恢复指令：读本文件 → iter83 续；npm test + cargo test -p alva-hub-desktop --lib


### 迭代压缩摘要
- iter1-13: 基础: V1 场景自动化/真实 readiness/.app 打包/跨引擎 memory/skills/MCP/测试基建
- iter14-17: 真实引擎: codex fresh+resume+fallback+worker, claude light, alva→codex handoff, 修 2 真 bug
- iter18-30: 前端测试全覆盖(bridge 100%)/hub-core/hub-cli 契约/打包产物
- iter31-40: 测试补全+质量门(每轮清理)
- iter41-43: UI 全面重设计(中性深色/单一 accent/去渐变)+sidebar 折叠
- iter44-48: wireframe 工具(CDP 零依赖)+抓 2 布局 bug+一键脚本
- iter49-58: 测试收尾+a11y(aria-expanded/aria-live/目标尺寸)
- iter59-61: ?demo=1 数据态+数据态审查+readiness 密度修复
- iter62-71: 测试补全+分步审查(--steps)+本地迭代+模型排查(内存守卫)
- iter72-80: 覆盖盘点/isolate/最小窗口/mxfp4 恢复(内存守卫根因)+视觉--steps
- iter81-88: 审查列标注改进(误判率降)+密集态验证+失败消息红调+summary 换行+徽章边框
- iter89-98: 审查循环稳定产出+质量门全绿(139 lib/30 web/ci 46)

## 会话 checkpoint 7（iter88 末）
全绿：hub-desktop 139 / hub-cli 25 / hub-core 21 / adapters 21 / web 29 / ci 46
审查闭环全通：mxfp4 文本+视觉(内存守卫根因已解)+列标注改进(误判率降)+--steps 双模式+?demo=1
审查累计修复(iter44-88)：topbar 撑高/sidebar 溢出/session-panel h=0/折叠 WCAG/aria/对比度/readiness 密度/占位符/status-pill aria-live/失败消息红调/状态徽章 12px/会话间距/composer summary 换行 等 15+ 项
模型最终配置：mxfp4(全) + ASR(语音)
恢复指令：读本文件 → iter89 续；npm test + cargo test -p alva-hub-desktop --lib

## 会话最终 checkpoint（iter92 末，压缩恢复点）
全量绿：hub-desktop 139 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 29 / ci 46
产品：V1 18 场景全自动化 + UI 全面重设计 + AI 审查驱动修复 15+ 项
工具链（全部验证可用）：scripts/ui-wireframe.sh / ui-review.sh(--steps) / ui-review-vision.sh(--steps --click) / model-verify.sh / web ?demo=1
模型：Qwen3.5-9B-mlx-vlm-mxfp4(文本+视觉主力, 内存守卫根因已解) + Qwen3-ASR-1.7B(语音)
关键文件：docs/UI-AI-TOOLCHAIN.md(工具+踩坑+模型迁移) / docs/ALVA-HUB-V1.md / .scratch/hub-loop/
恢复：读本文件 → iter93 续；每轮 npm test + cargo test -p alva-hub-desktop --lib；UI 改动走 wireframe+审查复核
