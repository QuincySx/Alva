# `alva plugins exec` —— Plugin 调试命令

> 对 Alva 最容易落地、性价比最高的 borrow 之一。

---

## 背景

详见 [`../plugins/debugging.md`](../plugins/debugging.md)。

核心价值：

```
Plugin 开发的老大难：
  写 plugin → 启动完整 agent → 触发对话 → 看 plugin 表现 → 改 → 重启 ...

有了 plugins exec：
  写 plugin → alva plugins exec <path> <event> --data '{...}' → 看输出 → 改 ...
```

反馈循环从几分钟缩到秒级。

---

## 建议实现

### Step 1：`alva plugins` 命令组

```rust
// alva-app-cli/src/cli/plugins.rs

use clap::{Parser, Subcommand};

#[derive(Parser)]
pub struct PluginsCmd {
    #[command(subcommand)]
    pub sub: PluginsSub,
}

#[derive(Subcommand)]
pub enum PluginsSub {
    /// List all discovered plugins with their status and registrations
    List,
    
    /// Trigger an event on a specific plugin (for testing)
    Exec {
        /// Plugin file path (e.g. .alva/plugins/my-plugin.py)
        plugin: PathBuf,
        
        /// Event name to trigger (tool.call / tool.result / agent.start / agent.end / configuration.change / event)
        event: String,
        
        /// JSON payload for the event
        #[arg(long, default_value = "{}")]
        data: String,
        
        /// Max wait time for async plugin handlers (seconds)
        #[arg(long, default_value = "2")]
        timeout: u64,
    },
}
```

### Step 2：`list` 实现

```rust
pub async fn handle_list(app: &App) -> Result<()> {
    let plugins = app.plugin_service.list_all().await?;
    
    for p in plugins {
        let rel = p.path.strip_prefix(std::env::current_dir()?)
            .unwrap_or(&p.path)
            .display()
            .to_string();
        
        let (mark, status_str) = match p.status {
            PluginStatus::Active => (style("✓").green(), "active"),
            PluginStatus::Loading => (style("…").yellow(), "loading"),
            PluginStatus::Error => (style("✗").red(), "error"),
        };
        
        println!("{} {} {}", mark, rel, style(status_str).dim());
        
        if !p.registered_events.is_empty() {
            println!("{}: {}", 
                style("  Events").dim(),
                style(p.registered_events.join(", ")).cyan());
        }
        for cmd in &p.registered_commands {
            println!("{}: {}", 
                style("  Command").dim(),
                style(format!("{}: {}", cmd.category, cmd.title)).cyan());
        }
        for tool in &p.registered_tools {
            println!("{}: {}", 
                style("  Tool").dim(),
                style(&tool.name).cyan());
        }
    }
    
    Ok(())
}
```

输出示例（要抄 Amp 的格式）：

```
✓ .alva/plugins/autolint.py           active
  Events: tool.result, agent.end
  Command: dev: Toggle linter
  Tool: run_linter

✓ .alva/plugins/rate-monitor.py       active
  Events: agent.start

✗ .alva/plugins/broken.py             error
```

### Step 3：`exec` 实现

这是重点。**用 stub host，不启动真 agent**。

```rust
pub async fn handle_exec(
    plugin: PathBuf,
    event: String,
    data: String,
    timeout_secs: u64,
) -> Result<()> {
    // 1. 解析 data
    let data: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| anyhow!("--data must be valid JSON: {}", e))?;
    
    // 2. 检查 plugin 文件存在
    if !plugin.exists() {
        bail!("Plugin file not found: {}", plugin.display());
    }
    
    // 3. 创建 stub host（最小接入面）
    let stub_host = StubHost::new();
    
    // 4. 启动 plugin subprocess
    let runner = PluginRunner::new_with_host(&plugin, Arc::new(stub_host))?;
    runner.start().await?;
    
    // 5. 触发事件
    runner.emit_event(&event, data).await?;
    
    // 6. 给 plugin 时间处理（异步 handler）
    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
    
    // 7. 清理
    runner.dispose().await?;
    
    Ok(())
}
```

### Step 4：Stub Host 实现

**关键**：只接入最小 RPC 表面，所有副作用（notify / open / thread.append）**打印到终端，不真执行**。

```rust
// alva-app-cli/src/cli/plugins_stub_host.rs

pub struct StubHost;

#[async_trait]
impl HostAPI for StubHost {
    async fn ui_notify(&self, message: String) -> Result<()> {
        println!("{} {}", style("[notify]").blue(), message);
        Ok(())
    }
    
    async fn ui_input(&self, options: InputOptions) -> Result<String> {
        eprintln!("{} {} (returning empty default)", 
            style("[input]").blue(),
            options.title);
        Ok(String::new())
    }
    
    async fn ui_confirm(&self, options: ConfirmOptions) -> Result<bool> {
        eprintln!("{} {} (returning false)", 
            style("[confirm]").blue(),
            options.title);
        Ok(false)
    }
    
    async fn ai_ask(&self, question: String) -> Result<AiAskResult> {
        eprintln!("{} {}", style("[ai.ask]").blue(), question);
        Ok(AiAskResult {
            result: "uncertain".into(),
            probability: 0.5,
            reason: "Stub host, AI not available".into(),
        })
    }
    
    async fn system_open(&self, url: String) -> Result<()> {
        println!("{} {}", style("[open]").blue(), url);
        Ok(())
    }
    
    fn system_amp_url(&self) -> Url {
        Url::parse("http://stub.local/").unwrap()
    }
    
    fn system_executor(&self) -> Executor {
        Executor { kind: "stub".into() }
    }
    
    async fn thread_append(&self, messages: Vec<Message>) -> Result<()> {
        for msg in messages {
            println!("{} {:?}", style("[thread.append]").blue(), msg);
        }
        Ok(())
    }
}
```

### Step 5：完整交互示例

```
$ alva plugins exec .alva/plugins/autolint.py tool.result --data '{
  "threadID": "T-test",
  "toolUseID": "tu-1",
  "name": "edit_file",
  "input": {"path": "/tmp/a.py", "old_str": "x", "new_str": "y"},
  "result": {"diff": "..."},
  "status": "done",
  "duration": 42
}'

[plugin] loading autolint.py ...
[plugin] loaded, ready
[plugin] event emitted: tool.result
[notify] Running linter on /tmp/a.py...
[ai.ask] Does this look like a root-cause fix?
[notify] Linter passed ✓
[plugin] handler returned in 1.2s
[plugin] dispose complete
```

---

## 集成点

- 新 CLI 模块：`alva-app-cli/src/cli/plugins.rs`
- 修改 `alva-app-core::PluginService`（或类似）暴露 `list_all()` 和 `PluginRunner::new_with_host()`
- 新 module：`alva-app-cli/src/cli/plugins_stub_host.rs`
- `alva plugins list` 和 `alva plugins exec` 子命令注册

---

## 隐藏收益

一旦有了 `plugins exec`：

1. **可以写单元测试脚本** 
   ```bash
   #!/bin/bash
   alva plugins exec ./autolint.py tool.result --data "$(cat fixtures/edit-call.json)" \
     > output.log
   grep "Linter passed" output.log || exit 1
   ```
2. **CI 可以验证 plugin 兼容性**
3. **Plugin 开发者不用了解 Alva 内部**就能自测
4. **重现 bug 变简单**：用户报 plugin 问题，直接跑同样的 event data 复现

---

## 优先级

**最高**。理由：

- 成本极低（几百行代码）
- 每个 plugin 开发者都会用
- 不改核心逻辑，零风险

**估算工作量**：1-2 天（包括 stub host 的所有 RPC 桩）。

---

## 扩展方向

### `alva plugins test <path>`

进一步抽象：plugin 作者可以在 plugin 旁边放 `tests.json`：

```json
[
  {
    "name": "edit_file triggers linter",
    "event": "tool.result",
    "data": { "name": "edit_file", "status": "done", ... },
    "expect": { "notify": ["Linter passed*"] }
  },
  ...
]
```

`alva plugins test autolint.py` 跑所有 test case，通过 = 0，失败 = 1。

### `alva plugins watch <path>`

文件改动自动重跑上次的 event。循环迭代再快。

这些可以后续按需加，不必 MVP 就做。
