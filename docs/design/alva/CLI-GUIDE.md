# SmallPen CLI 使用指南（新格式）

> 对象：`@smallpen/cli 0.2.0-alpha.2` 及之后，读写 `alva.smallpen`。更早的 JSON 格式包（`alva-foundation.smallpen` 和第一版 `alva.smallpen`）已被 SmallPen 废弃，并于 2026-11-09 从仓库删除，可在 git 提交 690c4525 找到；旧版 alpha.18~21 的命令（`page draw`、`project show` 等）已不存在。

## 先加载 Skill，再看帮助

```sh
smallpen help skills --json          # 返回 smallpen-ui-design/SKILL.md 的路径，读它
smallpen help rules start --json     # 规则索引；按需读 format、classes、tokens、components、themes
smallpen schema command view --json  # 单个命令的参数
```

Skill 管流程，本项目的风格、组件和业务取舍仍以 `DESIGN.md`、`COMPONENTS.md` 为准。命令参数以当前 `help`/`schema` 为准，不凭记忆。

## 包长什么样

```
alva.smallpen/
  index.html       包名、包 ID、主题默认值（<html data-appearance="light" data-viewport="desktop">）
  tokens.css       Token、主题、@font-face
  components/*.html  每个文件一个 <sp-component>：共享元素 + <sp-variant when="Kind: A; State: B">
  pages/<模块>/<页面>[.mobile|.tablet].html   页面：72 张，是包自己的正本，直接编辑（22 号的 .tablet 是 960 窄窗，不是平板设备）
  assets/fonts/    TTF 字体（Alva UI Sans SC、JetBrains Mono、Alva Interface Icons）
```

规则要点：只写 Tailwind v4 类，没有 `style`、`<style>`、`<script>`；元素用 `data-name` 命名，同级不重名；Token 是 `tokens.css` 里的 CSS 变量，前缀定类型（`--color-`、`--size-`、`--spacing-`、`--radius-`、`--border-`、`--shadow-`、`--text-`、`--font-`、`--font-weight-`、`--type-`、`--flag-`、`--string-`、`--opacity-`），类名是去掉前缀的名字（`--color-accent` → `bg-accent`、`text-accent`、`border-accent`）。没有 number 类型：`line.*`、`motion.*` 用 `--string-*` 携带。包与包不共享组件。

## 作者入口：不要手改组件 HTML

`alva.smallpen` 的 Token 与 56 个组件由 `source/build_next.py` 从 `design_system.py` 的 Python 定义生成（并叠加 `shell_deltas()` 里的原生外壳结构）。改设计 = 改 Python 源，再运行：

```sh
python3 -I docs/design/alva/source/build_next.py     # 重写 tokens.css、components/（删掉不再生成的组件文件）、token_names.json
python3 -I docs/design/alva/source/build_next.py --check   # 只比对：包与生成器不一致就失败（Go 测试在 CI 上也跑它）
smallpen check docs/design/alva/alva.smallpen --render
```

页面不由生成器产生，直接编辑 `pages/` 下的 HTML，改完 `smallpen check PKG --render --page "工作台/21 新任务" --all-themes`（明暗 × 宽窄）。

## 编辑后必须 `check`

```sh
smallpen check PKG                      # 通过的文件成为“已接受版本”，有错的文件被拒绝并列出
smallpen check PKG --render             # 另查文字放得下、越界、重叠、对比度；--page/--component 缩小范围
smallpen check PKG --revert [--file F]  # 把被拒绝的文件退回已接受版本
```

`check` 会写入“已接受版本”状态，只对要接受的新包运行，不要在别人正在 App 里打开的包上乱跑。`--all-themes` 耗时很长（超过 2 分钟），需要时放在能等待完成的地方。警告与渲染问题分开阅读：`check --render` 的发现不会让检查失败，但要逐条判断（本包当前的 29 条都是“只靠描边的控件对比度不足 3:1”，来自设计 Token，不是排版错误）。

## 看设计：spec、wireframe、PNG

```sh
smallpen view PKG --components --as text --limit 100 --stdout     # 组件目录：名称、轴、变体数
smallpen view PKG --component Button --variant "Kind: Primary; State: Default; Content: Text" --as spec --stdout
smallpen export PKG --component Button --format png --output DIR/Button   # 全部变体一张图，透明底
smallpen export PKG --component Button --format png --evidence --output DIR/Button  # spec + wireframe + PNG，同一次布局
```

- spec 给实现用：约束（`h-fit`、固定值）、Token 来源、覆盖项；按约束实现，不照抄解析出的像素高度。
- PNG 看外观；wireframe 看结构。三者用 `layout.hash` 对齐。
- 结果超过 8 KiB 会落到临时 `resultFile`，30 分钟后清理；要保留就用 `--output`，或加 `--stdout`。
- `export` 只写你指定的 `--output`，不会改包。

## 页面、原型与需求

- **canvas**：页面 `<body data-canvas="01 产品界面">` 分组；三个 canvas 是 `01 产品界面`（23）、`00 工作台线稿`（15）、`旧视觉草稿（暂不评审）`（34）。
- **原型**：起点是 `<body data-flow="名称">`（每个 canvas 一个），跳转是元素上的 `data-click="模块/页面"`，返回是 `data-click="back"`。连线只在同一个 canvas 内有效，`TestDesignPagesKeepTheirInventoryLinksAndRequirements` 守着：601 条连线都落在存在的同 canvas 页面上。组件实例内部的部件要链接时，写在 `<sp-use>` 里对应的覆盖元素上。
- **窄窗**：`--theme viewport/narrow` 只换 Token 值（宽 1024）；整页布局不同的才另写文件。22 号的 960 窄窗是 `22 对话与面板.tablet.html`，body 写死 960。
- **需求与标注**：新格式没有这两个对象。7 份需求在 `docs/design/alva/requirements/*.md`（正本），每条列出绑定的页面文件和 `data-name` 链；绑定过的页面顶部有一行 `<!-- 需求与标注见 … -->`，App 保存时可能丢掉这行，不影响正本。
- **来历**：这些页面由旧 JSON 包一次性转换而来（转换脚本已随旧包删除），现在页面就是包里的正本，直接编辑。

## 给应用代码导出 Token

```sh
smallpen token list PKG --limit 100 --stdout          # 类型、值、每个主题选项的值（values）
smallpen token export PKG --format json --by appearance --output DIR   # 每个选项一份完整文件
```

Alva Kit 的管线是 `source/export_tokens.py`：调用 `token list`，用 `source/token_names.json` 把 CSS 名映射回 Kit 名（`color-on-accent` → `color.onAccent`），写出 `apps/ui/kit/theme/tokens.json` 和 `apps/ui/kit/testdata/design-components.json`。包里出现表外的 Token、类型不符或缺 Token 都直接失败，不静默跳过。派生 Token（`type-*` 字体组合、`opacity-*`）不进 Kit。

## 设计稿对照

```sh
python3 -I docs/design/alva/source/export_design.py DIR [组件名 ...]   # DIR/<组件>.png，全部或指定
ALVA_KIT_DESIGN_DIR=DIR go run -tags desktop ./cmd/alva-kit-gallery        # 每页“设计稿 | 实现”两列
```

PNG 只留在本地，不提交。

## 接手清单

1. `smallpen help skills --json`，读 Skill；读 `DESIGN.md` 对应章节。
2. 改 Python 源 → `build_next.py` → `check --render` → 需要时 `export_design.py` 看 PNG。
3. 改了 Token 或组件目录：`python3 -I docs/design/alva/source/export_tokens.py` → `go generate ./apps/ui/kit/theme ./apps/ui/kit/icon` → `go test ./apps/ui/kit/...`，JSON 与生成文件同一次提交。
4. App 里保存过的改动是设计事实：先 `smallpen changes PKG --since REVISION`（每次读取都会报告 revision）再改，不要用生成器覆盖。生成器重写的是整个 `components/` 与 `tokens.css`，所以在 App 里直接改过组件的话，先把改动回写到 Python 源。

## 设计原则（沿用）

极简、克制、有少量细节；辅助文案只留影响选择的信息（来源、作用域、生效时机、权限边界、错误与禁用原因）。内容默认随内容定高（`h-fit`），高度 Token 只给有意的共享约束（控件 32、图标动作 28）。内容变化用实例覆盖，不为文案或记录增加变体；Token 组合表达数值差异，只有局部结构差异才用组件变体。包内 `w-full` 不能用在 hug 的父级上，生成器会自动把这种子级改为 `w-fit`。
