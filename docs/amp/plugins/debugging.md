# Plugin 调试

> `amp plugins` 子命令系列。对 Alva 最直接的启发之一。

---

## `amp plugins list`

列出当前工作区所有插件：

```
$ amp plugins list

✓ .amp/plugins/autolint.ts           active
  Events: tool.result, agent.end
  Command: dev: Toggle linter
  Tool: run_linter

✓ .amp/plugins/rate-limit-monitor.ts  active
  Events: agent.start

✗ .amp/plugins/broken.ts              error

✓ .amp/plugins/feature-flag.ts        active
  Events: configuration.change
```

### 实现

```js
plugins.list(async () => {
  let pluginsList = await g0(h.pluginService.plugins.pipe(
    Et(a => a.length > 0 && a.every(p => p.status !== "loading")),
    R$(100)
  ));
  
  let cwd = process.cwd();
  for (let c of pluginsList) {
    let absPath = c.uri.fsPath;
    let relPath = absPath.startsWith(cwd + "/") ? path.relative(cwd, absPath) : absPath;
    let mark = c.status === "active" ? chalk.green("✓") : chalk.red("✗");
    
    EI.write(`${mark} ${relPath} ${chalk.dim(c.status)}\n`);
    
    if (c.registeredEvents.length > 0) {
      EI.write(chalk.dim("  Events: ") + chalk.cyan(c.registeredEvents.join(", ")) + "\n");
    }
    if (c.registeredCommands.length > 0) {
      for (let cmd of c.registeredCommands) {
        EI.write(chalk.dim("  Command: ") + chalk.cyan(`${cmd.category}: ${cmd.title}`) + "\n");
      }
    }
    if (c.registeredTools.length > 0) {
      for (let tool of c.registeredTools) {
        EI.write(chalk.dim("  Tool: ") + chalk.cyan(tool.name) + "\n");
      }
    }
  }
});
```

**设计亮点**：
- 状态、路径、所有注册项**一眼看全**
- 异步等待加载完成（`status !== "loading"`）
- path 尽量显示相对路径

---

## `amp plugins exec <plugin> <event>`

**关键调试命令**：手动对 plugin 触发一个 event。

```bash
amp plugins exec .amp/plugins/autolint.ts tool.result --data '{
  "threadID": "T-test",
  "toolUseID": "tu-1",
  "name": "edit_file",
  "input": { "path": "/tmp/a.ts", "old_str": "x", "new_str": "y" },
  "result": { "diff": "..." },
  "error": null,
  "status": "done",
  "duration": 42
}'
```

### 实现

```js
plugins.exec(async (pluginPath, event, options) => {
  let data;
  try {
    data = JSON.parse(options.data);
  } catch {
    pu.write(chalk.red(`Error: --data must be valid JSON\n`));
    process.exit(1);
  }
  
  if (!existsSync(pluginPath)) {
    pu.write(chalk.red(`Error: plugin file not found: ${pluginPath}\n`));
    process.exit(1);
  }
  
  let resolved = path.resolve(pluginPath);
  let ampURL = parseURL(process.env.AMP_URL ?? DEFAULT_AMP_URL);
  
  // 创建孤立的 plugin runner，只接入最小 host stub
  let runner = new PluginRunner(URI.file(resolved), {
    onStderr: (c) => pu.write(c),
    onRequest: {
      "ui.notify": async ({ message }) => {
        pu.write(`[notify] ${message}\n`);
      },
      "system.open": async ({ url }) => {
        pu.write(`[open] ${url}\n`);
      },
      "client.info": async () => ({ ampURL, executorKind: "unknown" }),
      // 其他 RPC 可选，默认 undefined
    }
  });
  
  try {
    await runner.start();
    await runner.emitEvent(event, data);
    await sleep(2000);      // 等 plugin 处理
  } finally {
    await runner.dispose();
  }
  
  process.exit(0);
});
```

**设计亮点**：
- 不需要启动完整 Amp agent
- 只接入**最小 host stub**，`ui.notify` 只打印，`system.open` 不真开
- Plugin 的 stderr 直接转发到用户终端（真实调试体验）
- 有 2 秒 grace period 等 plugin 异步处理

---

## 这个命令为什么关键

Plugin 开发的老大难：

1. **不好调试**：要跑完整 agent、触发真实 event、才能看 plugin 表现
2. **反馈慢**：改一行 plugin 代码，要重启 agent、触发对话才能验
3. **副作用多**：真实 hook 可能触发写文件、调 API、改 thread

`amp plugins exec` 直接**短路**这些，给 plugin 开发者一个 local unit test runner。

开发流程变成：

```bash
# 在终端 1 写 plugin
vim .amp/plugins/my-plugin.ts

# 在终端 2 快速测
amp plugins exec .amp/plugins/my-plugin.ts tool.result --data '{...}'
# 看输出 → 改 → 再测，循环
```

---

## 对 Alva 的启发 ⭐⭐⭐

**这是整套 Amp 分析里对 Alva 最容易落地、收益最大的点之一。**

实现建议（Rust 伪代码）：

```rust
// alva-app-cli/src/cli/plugins.rs

#[derive(Parser)]
pub enum PluginsCmd {
    List,
    Exec {
        #[arg(help = "Path to .alva/plugins/*.py or *.ts")]
        plugin: PathBuf,
        
        #[arg(help = "Event name: tool.call / tool.result / agent.start / agent.end")]
        event: String,
        
        #[arg(long, help = "JSON event data")]
        data: String,
    },
}

pub async fn handle_plugins_cmd(cmd: PluginsCmd) -> Result<()> {
    match cmd {
        PluginsCmd::List => {
            let plugins = load_all_plugins(workspace_root()).await?;
            render_plugin_list(&plugins);
        }
        PluginsCmd::Exec { plugin, event, data } => {
            let data: serde_json::Value = serde_json::from_str(&data)?;
            let mut runner = PluginRunner::new_with_stub_host(plugin)?;
            runner.start().await?;
            runner.emit_event(&event, data).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            runner.dispose().await?;
        }
    }
    Ok(())
}
```

**Stub Host** 是关键 —— 只打印 notify / 不真开 URL / 拒绝 ai.ask（或用 mock 模型）。这让 plugin 测试**完全本地 + 零副作用**。

详细设计见 [`../alva-learnings/plugin-exec.md`](../alva-learnings/plugin-exec.md)。

---

## `amp plugins` 命令树总结

```
amp plugins
├── list                     # 列出所有 plugin + 状态 + 注册项
└── exec <plugin> <event>    # 手动触发一个 event
     --data <json>           # event payload
```

Amp 没看到其他 plugins 子命令。但从实现看可能还有：

- `amp plugins enable / disable <name>` —— 可能用 settings.json 控制
- `amp plugins install <url>` —— 未观察到，可能未实现

---

## 调试 Tips（基于反编译信息）

1. **Plugin 必须带 "acknowledgment comment"** 才被加载：
   ```
   Plugin not loaded: <path>. Missing required acknowledgment comment.
   ```
   推测是文件顶部有 `// @ampcode-plugin: true` 之类的 marker。

2. **Plugin 的 stderr 统一转发到主进程 stderr** —— 用 `console.error` / `process.stderr.write` 是标准 debug 方式。

3. **Plugin 抛异常会被降级** —— 不会让主 agent 挂，但会看不到原因。建议 plugin 内 try-catch + `logger.debug`。

4. **OTEL span 在每个 hook** —— 生产环境问题可以从 trace 定位到具体 plugin。
