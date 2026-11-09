# ADR 0014 Alva Kit 组件库

- Status: Accepted
- Date: 2026-11-09
- Amends: `docs/design/alva/COMPONENTS.md` 中“不要求 Go／HTML 再发布一个独立 UI 包”的说法

用户 2026-11-09 决定：参照 LobeHub 的 lobe-mygo（MyGo 上的组件库），先做一套**我们自己的**组件库，
把这层抽象做好，后续界面开发由它加速。视觉风格来自我们自己的设计稿；lobe-mygo 只作为组件交互与传参的参考。

## 背景

- 设计稿已有完整组件目录：56 个 Alva 组件、481 个变体，61 个 token（`docs/design/alva/source/design_system.py` 的 `token_definitions()`）。
- 代码里没有对应的组件层：`apps/hub/desktop` 用 17 个私有 `workspaceControl*` 函数（约 400 处调用）、86 处原生 `ui.Button`、
  各自手写的浮层壳与行，token 字面量散在三处，浮层阴影已漂移成三种且都不是 token 值，设计的 68 个图标缺 20 个。
- lobe-mygo（MIT）依赖 MyGo 0.8.4 预发布版，用到 0.7.4 没有的 `MenuBase` 等；它的 Button 没有 Disabled／Loading／焦点环，
  图标按钮名称可选，也不处理减弱动效。这些恰是我们设计稿的硬要求。

## 决定

1. **Alva Kit**：新建 `apps/ui/kit`（包名 `kit`），是 MyGo `*Base` 控件之上的薄皮肤。不对外独立发布，留在本 module；
   MyGo 保持 ADR 0012 固定的 v0.7.4，缺的能力（如菜单的 roving focus）由 kit 自写。
2. **借鉴范围**：从 lobe-mygo 借 API 形状、测试方法与画廊组织，不搬实现；若复制代码，在 `NOTICE` 记 LobeHub MIT 条目。
3. **API 约定**：即时模式，`Name(c, 数据, NameProps) ui.Element`；Props 零值即设计默认；设计的配置轴是枚举，
   状态轴只有 Disabled／Busy／Invalid／ReadOnly 进 Props（Hover／Pressed／Focus 来自运行时）；单一密度、无尺寸档；
   受控值用指针；动作用受门控的 `OnClick`；图标按钮的可访问名称是必填位置参数；组件内不出现颜色与尺寸字面量。
4. **token 单一来源**：设计源 `token_definitions()` 离线导出为 JSON，Go 生成器产出 `apps/ui/kit/theme` 的代码，二者一并提交，
   测试保证一致；Go 构建不依赖 Python。图标名称表同理由设计的图标清单生成。
5. **画廊与测试**：`cmd/alva-kit-gallery` 按设计轴展示每个组件的全部变体与明暗主题，作为视觉审阅入口；每个组件有交互、
   几何／像素与截图测试；截图供人工审阅，不提交 PNG 基线。覆盖闸要求设计目录里的组件要么在画廊，要么在只减不增的延后清单里。
6. **分层**：kit 只依赖标准库、MyGo 和自身；桌面与 `apps/ui/chat|modules` 依赖 kit，反向禁止。新增只减不增的
   `kit_adoption` 架构测试，阻止桌面代码继续新增颜色字面量、原生按钮与自制浮层壳。
7. **分期**：K1 基础控件 → K2 浮层、反馈、布局与数据展示 → K3 Alva 业务组合（`kit/blocks`，不接产品类型）→
   K4 迁移桌面端（M2 完成后，归入 M6）。

## 后果

- 设计改 token 或组件后，有明确的同步步骤与失败的测试指引；界面一致性由 kit 和架构测试守护，而不是各页面自觉。
- 菜单等需要自写键盘与焦点逻辑，是 K1 的主要风险；兜底是 MyGo 自带的 `PopoverMenu`。
- `COMPONENTS.md` 的旧说法、`DESIGN.md` 中已被取代的尺寸与字级旧值，随 K1 订正。

否决：直接依赖 lobe-mygo。原因：需要升级 MyGo 到预发布版，视觉是 Lobe UI 而非我们的设计，且缺少设计稿要求的状态与可访问性。
