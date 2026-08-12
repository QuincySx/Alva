# 03 Token, Density, and Motion Policy

## 目标

把 ALVA 深色语义、default/compact 两档密度和 Emil Skill 动效原则固化成代码规则。

## 工作

- 建立 color/type/space/radius/shadow/size/layer/motion 语义 token；
- 所有 token 为后续浅色映射保留语义边界；
- 增加 default/compact 控件与列表尺寸；
- 增加 press/tooltip/disclosure/popover/dialog/drawer duration；
- 增加强 ease-out、ease-in-out、drawer curve；
- 删除 UI 中的 `ease-in`、`transition: all` 和散落时长；
- 区分 pointer active、keyboard activation 和 reduced motion；
- 添加 CSS 架构与 motion 静态检查。

## 验收

- 页面无共享魔法常量和自定义控件高度；
- UI 进入/退出不使用 `ease-in`；
- 常规 UI 动画均小于 300 ms；
- 高频导航无位移或缩放动画；
- 深色 token 不阻塞未来浅色语义映射。
