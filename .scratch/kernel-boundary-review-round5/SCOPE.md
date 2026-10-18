# 全 agent / ticket 覆盖清单

用户最初要求继续审查，随后明确“所有都需要审查”。因此除最新11提交外，按当前最终实现展开以下全部可发现任务清单，共106张历史/当前卡。

| 清单 | 卡数 | 审查文档 |
|---|---:|---|
| kernel-boundary 01–19 | 19 | HISTORICAL-CORE.md |
| kernel-boundary-review-fixes R01–R08 | 8 | HISTORICAL-CORE.md |
| review-round2 R09–R13 | 5 | HISTORICAL-CORE.md |
| review-round3 R14–R23、UI01–UI10 | 20 | CORE中的R14–19 + UI中的R20–23/UI01–10 |
| review-round4 R24–R37 | 14 | REVIEW.md |
| hub-engine-seam 01–18 | 18 | HISTORICAL-SEAM.md |
| alva-hub-tauri-ui-system 00–12 | 13 | HISTORICAL-UI.md |
| alva-hub-v1-release-readiness 00–08 | 9 | HISTORICAL-UI.md（待开始/预检，不误作完成实现） |
| 合计 | 106 | 每卡有当前状态与证据界限 |

## 分支与工作区

`git worktree list --porcelain`：只有当前工作区，HEAD `5c64af1640ab00b6a64aa3474112b62fe4106036`。

`main`、`feat/headless-jobs`、`feat/headless-v2`、`feat/provider-discovery`、`feat/recursion-gate`、`fix/subagent-spawn-deadlock-cancel` 都是HEAD祖先。

`backup-before-date-rewrite` 有17个不同提交号；`git cherry HEAD backup-before-date-rewrite` 17项全为 `-`，补丁等价，没有额外未合入实现。远程追踪main/fix亦已包含。未fetch远程或声称检查机器之外未提供的分支。

当前原有未跟踪资料不视为生产变更，不删除、不覆盖。历史中间commit没有逐个重建；审查所有可发现ticket的最终代码与证据，不冒称逐行重新验证仓库整个历史。
