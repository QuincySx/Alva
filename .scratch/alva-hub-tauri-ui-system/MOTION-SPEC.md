# Motion Specification

> The accepted project standard now lives in
> `docs/ALVA-HUB-UI-MOTION.md`. This scratch copy remains the implementation
> worksheet for the current redesign.

## 动效决策

每个动效按顺序回答：

1. 用户多久看到一次？
2. 动效用于反馈、空间关系、状态解释，还是仅装饰？
3. 元素是进入/退出、屏内移动、hover/color，还是持续运动？
4. 是否可以被快速反向操作？
5. reduced motion 下保留什么信息？

没有明确目的就不动画。

## Token

```css
--duration-press: 120ms;
--duration-tooltip: 140ms;
--duration-popover: 180ms;
--duration-dialog: 220ms;
--duration-drawer: 240ms;

--ease-out: cubic-bezier(0.23, 1, 0.32, 1);
--ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
--ease-drawer: cubic-bezier(0.32, 0.72, 0, 1);
```

不提供 `ease-in` token。退出同样快速响应，使用更短的 ease-out 或对称 transition。

## 组件动效矩阵

| 组件 | 频率 | 动效 | 时长/曲线 | reduced motion |
|---|---:|---|---|---|
| Button/IconButton | 高频 | pointer press `scale(.97)`；键盘无位移 | 120ms ease-out | 仅颜色 |
| Sidebar/Nav/Tab | 高频 | 只做颜色/背景；不缩放、不滑入 | 100–140ms ease | 保留颜色 |
| Tooltip | 中高频 | 首次 opacity + scale(.97)；连续 tooltip 立即显示 | 140ms ease-out | opacity |
| Select/Menu/Popover | 偶发 | 从 trigger origin opacity + scale(.98) | 180ms ease-out | opacity |
| Dialog | 偶发 | 中心 opacity + scale(.985)，背景淡入 | 220ms ease-out | opacity |
| Workbench/Sheet | 偶发 | 与来源方向一致的 translateX | 240ms drawer curve | opacity 或瞬时 |
| Disclosure | 中频 | chevron rotate；内容用可中断过渡 | 160–200ms ease-out | 内容瞬时，箭头保留 |
| Switch | 中频 | thumb translateX + track color | 160ms ease-out | 保留状态变化 |
| Spinner | 等待 | linear rotation | 700–900ms linear | 保留 |
| Skeleton | 等待 | 低对比 shimmer，仅较长等待 | 1200ms linear | 静态骨架 |
| Toast/Notice enter | 偶发 | opacity + 同来源方向轻移 | 180–220ms ease-out | opacity |

## 不动画

- 键盘触发的 command/search 切换；
- 高频 Sidebar 和 session 列表导航；
- 表格、日志、token 数字的普通刷新；
- 自动滚动中的每条流式 token；
- 大面积背景和装饰光效；
- 布局属性 `width/height/margin/padding/top/left` 的逐帧动画。

## 当前代码审计

| Before | After | Why |
|---|---|---|
| Popover/Dialog 退出使用 `ease-in` | 统一短时 `ease-out` transition | `ease-in` 起步慢，关闭显得迟钝 |
| Popover/Dialog/Disclosure 使用 keyframes | 可快速反向的组件改用 transition/data-state | keyframes 被打断时从头播放 |
| NavItem/Tab 高频点击缩放 | 移除缩放，只保留颜色反馈 | 高频导航不应有重复位移动效 |
| Button 仅 `scale(.99)` | pointer press 使用 `scale(.97)` | 提供明确但克制的按压反馈 |
| hover 动效对所有指针生效 | 用 `(hover: hover) and (pointer: fine)` 包裹 | 避免触屏 tap 后残留 hover |
| reduced motion 全部 transition 清零 | 仅移除 transform/position，保留颜色/opacity | 减弱动效仍需传达状态 |
| Overlay 时长只有 fast/normal | 按 press/tooltip/popover/dialog/drawer 分类 | 时长应匹配组件尺寸和频率 |
| Disclosure 动画 `height` | 优先 grid-template-rows 或 Radix height + transition 方案 | 减少布局抖动并支持反向操作 |

## 验收

- DevTools 以 0.25× 慢放检查进入、退出和快速反向；
- 录制 60 FPS trace，常用交互不得出现明显 dropped frame；
- 鼠标、键盘、触屏分别检查 hover/active/focus；
- `prefers-reduced-motion: reduce` 下逐状态截图；
- Tooltip 连续浏览无重复延迟和动画；
- Popover 的 `transform-origin` 与 trigger 对齐，Dialog 保持中心 origin。
