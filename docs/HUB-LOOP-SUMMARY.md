# ALVA Hub 演进循环成果（iter1-156 总结）

## 产品完成度
- V1 18 个验收场景全部自动化（hub-desktop 139 lib / hub-cli 25 / hub-core 21 / adapters 21 / web 32 / ci 46）
- 真实引擎打包运行全覆盖：alva(4) / codex(fresh+resume+fallback+worker) / claude light(14) / alva→codex handoff
- UI 全面重设计（中性深色/单一 accent/无渐变）+ AI 审查驱动修复 20+ 项

## 关键修复清单（AI 审查驱动）
topbar 撑高 / sidebar 溢出 / session-panel h=0 / 折叠 WCAG+aria-expanded / 对比度 4.88:1 /
readiness 密度 / 占位符可读性 / status aria-live / aria-current / failed 红调 / 徽章 12px+border /
summary 换行 / footer 间距 / 面板 aria-labelledby / session 相对时间+结果 等

## 工具链（全部可用）
- scripts/ui-wireframe.sh —— 文本线稿（CDP，零依赖）
- scripts/ui-review.sh --steps —— 文本审查（分步绕长输出）
- scripts/ui-review-vision.sh --steps --click —— 视觉审查（截图+交互）
- scripts/model-verify.sh —— 换模型验货
- web ?demo=1 —— dev 数据态（审查用）

## 模型（最终配置）
- Qwen3.5-9B-mlx-vlm-mxfp4 —— 文本+视觉主力（内存守卫根因已解）
- Qwen3-ASR-1.7B —— 语音
- 踩坑：长输出卡 = oMLX 内存守卫；审查前 curl 验 dev server；负反馈与代码事实核对

## 审查循环经验
三维度正向通过后，主观负面反馈按"与代码事实核对"原则甄别（模型有波动）。
恢复点：.scratch/hub-loop/STATE.md（12 checkpoint + 范围摘要）
