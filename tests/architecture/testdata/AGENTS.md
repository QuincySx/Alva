# Architecture acceptance data
> 可审核的 SDK 公开面快照，不在运行期读取。

## 地位
仅测试数据，无产品、宿主或SDK runtime依赖。

## 逻辑
public-surface由AST机械生成，并在接口审查后显式更新。

## 约束
不得为了让CI绿而无解释更新；本目录不保存凭据或用户数据。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| public-surface | public-surface/ | wire、bus、kernel/agent ABI、可替换kernel core/装配与共享storage/blob公开面 |
