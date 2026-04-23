# 反编译方法论

> 记录怎么从 Amp 的闭源二进制里把内容挖出来，供未来对其他类似产品做同样分析时参考。

---

## 目标二进制

```
$ file ~/.amp/bin/amp
Mach-O 64-bit executable arm64

$ ls -l ~/.amp/bin/amp
-rwxr-xr-x 69952784  /Users/.../.amp/bin/amp
```

70 MB 的 single-file 可执行。

## 关键洞察：Bun compile 不混淆 JS 源

Amp 用 `bun build --compile` 打包，而 Bun 把 JS/TS 源码**明文嵌进二进制尾部**。这意味着：

- 不需要真正的反编译器（`jadx` / `Ghidra` / `IDA`）
- `strings(1)` 就能把所有模板字符串、函数名、常量提取出来
- 唯一的障碍是 minifier 把变量名改成了 `T`/`R`/`$iT`/`Y8` 这样的短符号

## 提取流程

```bash
# 1. dump 所有可打印字符串（过滤长度 >= 20 的）
mkdir -p /tmp/amp-decompile
strings -n 20 /Users/smallraw/.amp/bin/amp > /tmp/amp-decompile/strings.txt

# 结果：
wc -l /tmp/amp-decompile/strings.txt
# 66622 lines, ~13 MB
```

## 定位关键区域

用标识字符串定位 Amp 自己的代码（不是 Bun/Node 运行时）：

```bash
# Amp 代码集中在 62300-66000 这个区间
grep -n "You are Amp" strings.txt          # 主 prompt 入口
grep -n "ampcode.com" strings.txt          # Amp 特有 URL
grep -n "inputSchema:" strings.txt         # tool 定义
```

## 分类拆分

根据大致的行号范围，把 strings.txt 切成几份便于查阅：

| 文件 | 行号范围 | 内容 |
|---|---|---|
| `main_prompts.txt` | 62519-63005 | fwR/kwR/$wR/EwR/MwR/DwR/wwR 7 个主 system prompt |
| `oracle.txt` | 63232-63272 | Oracle 子 agent prompt |
| `reviewer_diffexp.txt` | 64435-64495 | Code reviewer + Diff explainer |
| `librarian_analyzer.txt` | 64619-64683 | Librarian + File analyzer |
| `walkthrough.txt` | 64689-64720 | Walkthrough 三阶段 prompt |
| `tools/*` | 65100-66000 | 各工具 spec |

工具具体拆分由 `awk 'NR>=X && NR<=Y'` 完成。

## 变量名解码

Minifier 把标识符改成了单字母 / 短 hash，但**语义字符串保留了原文**。解码原理：

```js
// 原本可能是：
{ spec: { name: "Bash", description: BASH_DESCRIPTION, ... }, fn: bashFn }

// 压缩后：
{ spec: { name: Y8, description: M2R, ... }, fn: HzT }
```

通过阅读 prompt 里的上下文反推变量 → 工具名映射：

```
${Y8} = Bash          (prompt 里讲 shell 工具时提到)
${P8} = Read          (prompt 里讲读文件时提到)
${dr} = edit_file     (prompt 里讲 str-replace 编辑时提到)
${we} = create_file
${ee} = Grep
${vt} = finder (codebase_search_agent)
${wr} = oracle
${he} = Task
${ly} = read_web_page
${r$} = web_search
${ui} = read_github (Librarian)
${U7} = chart
${q0T} = undo_edit
${Ot} = AGENTS.md
${VW} = image_generation
${W0T} = mermaid
${is} = load_skill
${H0T} = todo_write
...
```

完整解码表见 [`prompts/placeholder-dictionary.md`](./prompts/placeholder-dictionary.md)。

## 工具 Spec 发现

工具定义以对象字面量形式嵌入：

```js
L2R = {
  spec: {
    name: Y8,                          // "Bash"
    description: M2R,                  // 大段文档（也是字符串常量）
    inputSchema: { type:"object", properties:{...}, required:[...] },
    source: "builtin",
    meta: { disableTimeout: true },
    executionProfile: { serial: true, resourceKeys: ()=>[] }
  },
  fn: HzT,                             // handler 函数
  preprocessArgs: (T,R) => { ... }     // 可选：参数预处理
}
```

用 `grep '{spec:{name:'` 定位所有工具，然后 `awk` 取上下文就能还原完整 spec。

## 局限

**能还原的**：
- 所有 prompt 字符串（模板字面量）
- 所有工具的 `name` / `description` / `inputSchema`
- 工具间的符号引用（通过 `${xxx}` 占位符间接）
- 所有 URL 和 API endpoint 形式

**不能还原的**：
- 函数内部的控制流细节（minifier 重命名了局部变量）
- 类型信息（TypeScript 已经擦除）
- 服务器端逻辑（只在 ampcode.com 后端，二进制里看不到）

**推测依赖交叉验证的**：
- DTW 的具体实现（只看到 `Cloudflare Logs` / `Data Studio` 等字样，推断是 Durable Objects）
- Aggman 的实际 UI（只看到系统 prompt，没看到前端代码）

## 其他产物

- `/tmp/amp-decompile/README.md` —— 中间产物目录的导读
- `/tmp/amp-decompile/prompts_raw.txt` —— 未经整理的 prompt 原文行
- `/tmp/amp-decompile/strings.txt` —— 完整 strings 输出

这些是**工作区文件**，不应该 commit 到仓库。本目录 `docs/amp/` 下的所有内容都是已提炼的结构化知识。
