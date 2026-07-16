// INPUT:  rquickjs, WasiFs, crate::http_proxy, alva_{kernel_abi,sandbox_abi} contracts, async_trait, serde, serde_json, std::{path, sync, time}
// OUTPUT: RunScriptTool
// POS:    Guest-only QuickJS tool with bounded execution, capability-confined files, and synchronous host-policy fetch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alva_agent_extension_builtin::WasiFs;
use alva_kernel_abi::{AgentError, ExecutionMode, Tool, ToolExecutionContext, ToolOutput};
use alva_sandbox_abi::{FetchHeader, FetchRequest};
use async_trait::async_trait;
use rquickjs::{
    allocator::RustAllocator, CatchResultExt, Coerced, Context, Ctx, Exception, Function, Runtime,
};
use serde::Deserialize;
use serde_json::json;

/// Wall-clock budget for one script, when the caller does not set one.
///
/// This has to stay comfortably under the host's `RunLimits::wall_clock`
/// backstop (30s by default): QuickJS's interrupt handler kills only the
/// script and hands the agent a readable error, while the host epoch traps the
/// whole guest. The script limit is meant to fire first — but the point of
/// `run_script` is editing N files in one call, so the budget also has to be
/// big enough for real batches.
const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_SCRIPT_MEMORY_BYTES: usize = 32 * 1024 * 1024;
const SCRIPT_STACK_BYTES: usize = 512 * 1024;

/// Per-script resource ceilings, owned by the caller so the guest's limits can
/// be tuned alongside the host's `RunLimits` instead of being frozen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLimits {
    pub timeout: Duration,
    pub memory_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_SCRIPT_TIMEOUT,
            memory_bytes: DEFAULT_SCRIPT_MEMORY_BYTES,
        }
    }
}

const BINDING_BOOTSTRAP: &str = r#"
(() => {
  const readFileRaw = globalThis.__alvaReadFile;
  const writeFileRaw = globalThis.__alvaWriteFile;
  const appendFileRaw = globalThis.__alvaAppendFile;
  const existsRaw = globalThis.__alvaExists;
  const removeRaw = globalThis.__alvaRemove;
  const renameRaw = globalThis.__alvaRename;
  const mkdirRaw = globalThis.__alvaMkdir;
  const readDirRaw = globalThis.__alvaReadDir;
  const statRaw = globalThis.__alvaStat;
  const globRaw = globalThis.__alvaGlob;
  const copyFileRaw = globalThis.__alvaCopyFile;
  const printRaw = globalThis.__alvaPrint;
  const fetchRaw = globalThis.__alvaFetch;

  const normalizeHeaders = (headers = {}) => {
    if (Array.isArray(headers)) {
      return headers.map((entry) => {
        if (!Array.isArray(entry) || entry.length !== 2) {
          throw new TypeError("fetch headers array entries must be [name, value]");
        }
        return { name: String(entry[0]), value: String(entry[1]) };
      });
    }
    if (headers === null || typeof headers !== "object") {
      throw new TypeError("fetch headers must be an object or [name, value][]");
    }
    return Object.entries(headers).map(([name, value]) => ({
      name: String(name), value: String(value)
    }));
  };

  Object.assign(globalThis, {
    readFile: (path) => readFileRaw(String(path)),
    writeFile: (path, content) => writeFileRaw(String(path), String(content)),
    appendFile: (path, content) => appendFileRaw(String(path), String(content)),
    exists: (path) => existsRaw(String(path)),
    remove: (path) => removeRaw(String(path)),
    rename: (from, to) => renameRaw(String(from), String(to)),
    mkdir: (path) => mkdirRaw(String(path)),
    readDir: (path = ".") => JSON.parse(readDirRaw(String(path))),
    stat: (path) => JSON.parse(statRaw(String(path))),
    glob: (pattern) => JSON.parse(globRaw(String(pattern))),
    copyFile: (from, to) => copyFileRaw(String(from), String(to)),
    readJson: (path) => JSON.parse(readFileRaw(String(path))),
    writeJson: (path, value, space = 2) =>
      writeFileRaw(String(path), JSON.stringify(value, null, space)),
    print: (...values) => printRaw(values.map((value) =>
      typeof value === "string" ? value : JSON.stringify(value)
    ).join(" ")),
    fetch: (url, init = {}) => {
      if (init === null || typeof init !== "object") {
        throw new TypeError("fetch init must be an object");
      }
      const raw = fetchRaw(
        String(init.method === undefined ? "GET" : init.method),
        String(url),
        JSON.stringify(normalizeHeaders(init.headers)),
        String(init.body === undefined || init.body === null ? "" : init.body)
      );
      const response = JSON.parse(raw);
      return Object.freeze({
        status: response.status,
        ok: response.status >= 200 && response.status < 300,
        headers: Object.freeze(response.headers),
        body: response.body,
      });
    },
  });

  for (const name of [
    "__alvaReadFile", "__alvaWriteFile", "__alvaAppendFile", "__alvaExists",
    "__alvaRemove", "__alvaRename", "__alvaMkdir", "__alvaReadDir",
    "__alvaStat", "__alvaGlob", "__alvaCopyFile", "__alvaPrint", "__alvaFetch"
  ]) delete globalThis[name];
})();
"#;

#[derive(Debug, Deserialize)]
struct Input {
    /// JavaScript source evaluated as a strict global script.
    script: String,
}

/// Runs one source string in a fresh QuickJS runtime.
pub struct RunScriptTool {
    fs: Arc<WasiFs>,
    limits: ScriptLimits,
}

impl RunScriptTool {
    pub fn new(workspace: impl Into<std::path::PathBuf>) -> Self {
        Self {
            fs: Arc::new(WasiFs::new(workspace)),
            limits: ScriptLimits::default(),
        }
    }

    pub fn with_limits(mut self, limits: ScriptLimits) -> Self {
        self.limits = limits;
        self
    }

    fn run(&self, script: &str) -> Result<ScriptResult, ScriptFailure> {
        // WASI libc does not expose a useful `malloc_usable_size`, so the
        // default QuickJS allocator under-counts heap use. RustAllocator keeps
        // the requested size in an allocation header, letting QuickJS enforce
        // its per-runtime memory ceiling before the host's linear-memory trap.
        let runtime = Runtime::new_with_alloc(RustAllocator).map_err(|error| {
            ScriptFailure::without_output(format!("initialize QuickJS: {error}"))
        })?;
        runtime.set_memory_limit(self.limits.memory_bytes);
        runtime.set_max_stack_size(SCRIPT_STACK_BYTES);

        let started = Instant::now();
        let timeout = self.limits.timeout;
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_by_handler = Arc::clone(&interrupted);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let expired = started.elapsed() >= timeout;
            if expired {
                interrupted_by_handler.store(true, Ordering::Relaxed);
            }
            expired
        })));

        let context = Context::full(&runtime).map_err(|error| {
            ScriptFailure::without_output(format!("initialize QuickJS context: {error}"))
        })?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let result = context.with(|ctx| {
            install_bindings(&ctx, Arc::clone(&self.fs), Arc::clone(&output))
                .map_err(|error| format!("install file bindings: {error}"))?;
            ctx.eval::<(), _>(BINDING_BOOTSTRAP)
                .catch(&ctx)
                .map_err(|error| format!("initialize file bindings: {error}"))?;

            let value = ctx
                .eval::<Coerced<String>, _>(script)
                .catch(&ctx)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(value.0)
        });

        let captured_output = output.lock().expect("script output lock").clone();
        let failure = |error| ScriptFailure {
            error,
            output: captured_output.clone(),
        };
        match result {
            Ok(value) => Ok(ScriptResult {
                result: value,
                output: captured_output,
            }),
            Err(_error) if interrupted.load(Ordering::Relaxed) => Err(failure(format!(
                "script timed out after {} ms",
                timeout.as_millis()
            ))),
            Err(error) if is_memory_error(&error) => Err(failure(format!(
                "script exceeded the {} MiB memory limit: {error}",
                self.limits.memory_bytes / (1024 * 1024)
            ))),
            Err(error) => Err(failure(format!("script evaluation failed: {error}"))),
        }
    }
}

#[derive(Debug)]
struct ScriptResult {
    result: String,
    output: Vec<String>,
}

#[derive(Debug)]
struct ScriptFailure {
    error: String,
    output: Vec<String>,
}

impl ScriptFailure {
    fn without_output(error: String) -> Self {
        Self {
            error,
            output: Vec::new(),
        }
    }
}

fn is_memory_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("alloc") || error.contains("memory")
}

fn js_error<'js, T>(ctx: &Ctx<'js>, result: Result<T, AgentError>) -> rquickjs::Result<T> {
    result.map_err(|error| Exception::throw_message(ctx, &error.to_string()))
}

fn install_bindings<'js>(
    ctx: &Ctx<'js>,
    fs: Arc<WasiFs>,
    output: Arc<Mutex<Vec<String>>>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaReadFile",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            let bytes = js_error(&ctx, bound.read_file_sync(&path))?;
            String::from_utf8(bytes)
                .map_err(|error| Exception::throw_message(&ctx, &format!("{path}: {error}")))
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaWriteFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, path: String, content: String| {
                js_error(&ctx, bound.write_file_sync(&path, content.as_bytes()))?;
                Ok::<_, rquickjs::Error>(content.len() as i32)
            },
        )?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaAppendFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, path: String, content: String| {
                js_error(&ctx, bound.append_file_sync(&path, content.as_bytes()))?;
                Ok::<_, rquickjs::Error>(content.len() as i32)
            },
        )?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaExists",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            js_error(&ctx, bound.exists_sync(&path))
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaRemove",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            js_error(&ctx, bound.remove_sync(&path))?;
            Ok::<_, rquickjs::Error>(())
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaRename",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, from: String, to: String| {
                js_error(&ctx, bound.rename_sync(&from, &to))?;
                Ok::<_, rquickjs::Error>(())
            },
        )?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaMkdir",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            js_error(&ctx, bound.create_dir_all_sync(&path))?;
            Ok::<_, rquickjs::Error>(())
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaReadDir",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            let entries = js_error(&ctx, bound.list_dir_sync(&path))?;
            serde_json::to_string(
                &entries
                    .into_iter()
                    .map(|entry| {
                        json!({"name": entry.name, "isDir": entry.is_dir, "size": entry.size})
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaStat",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, path: String| {
            let metadata = js_error(&ctx, bound.metadata_sync(&path))?;
            Ok::<_, rquickjs::Error>(
                json!({
                    "isFile": metadata.is_file,
                    "isDir": metadata.is_dir,
                    "size": metadata.size,
                    "readonly": metadata.readonly,
                })
                .to_string(),
            )
        })?,
    )?;

    let bound = Arc::clone(&fs);
    globals.set(
        "__alvaGlob",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, pattern: String| {
            let matches = js_error(&ctx, bound.glob_sync(&pattern))?;
            serde_json::to_string(&matches)
                .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
        })?,
    )?;

    globals.set(
        "__alvaCopyFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, from: String, to: String| {
                let bytes = js_error(&ctx, fs.copy_file_sync(&from, &to))?;
                i32::try_from(bytes).map_err(|_| {
                    Exception::throw_message(&ctx, "copied file size exceeds script integer range")
                })
            },
        )?,
    )?;

    globals.set(
        "__alvaPrint",
        Function::new(ctx.clone(), move |line: String| {
            output.lock().expect("script output lock").push(line);
        })?,
    )?;

    globals.set(
        "__alvaFetch",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>,
                  method: String,
                  url: String,
                  headers_json: String,
                  body: String| {
                let headers: Vec<FetchHeader> =
                    serde_json::from_str(&headers_json).map_err(|error| {
                        Exception::throw_message(&ctx, &format!("invalid fetch headers: {error}"))
                    })?;
                let response = crate::http_proxy::fetch(FetchRequest::new(
                    method,
                    url,
                    headers,
                    body.into_bytes(),
                ))
                .map_err(|error| Exception::throw_message(&ctx, &error))?;
                let body = String::from_utf8(response.body).map_err(|error| {
                    Exception::throw_message(
                        &ctx,
                        &format!("fetch response body is not valid UTF-8: {error}"),
                    )
                })?;
                serde_json::to_string(&json!({
                    "status": response.status,
                    "headers": response.headers,
                    "body": body,
                }))
                .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
            },
        )?,
    )?;

    Ok(())
}

#[async_trait]
impl Tool for RunScriptTool {
    fn name(&self) -> &str {
        "run_script"
    }

    fn description(&self) -> &str {
        "Run JavaScript in a bounded QuickJS runtime. No modules, npm, Node APIs, require, or ambient filesystem are available; use the documented file bindings and synchronous host-allowlisted fetch. Fetch failures are catchable JavaScript exceptions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "JavaScript source to execute as one global script"
                }
            },
            "required": ["script"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let input: Input =
            serde_json::from_value(input).map_err(|error| AgentError::ToolError {
                tool_name: self.name().to_string(),
                message: format!("invalid input: {error}"),
            })?;
        match self.run(&input.script) {
            Ok(result) => Ok(ToolOutput {
                content: vec![alva_kernel_abi::ToolContent::text(
                    json!({"result": result.result, "output": result.output}).to_string(),
                )],
                is_error: false,
                details: None,
            }),
            Err(failure) => Ok(ToolOutput::error(
                json!({
                    "error": format!("run_script failed: {}", failure.error),
                    "output": failure.output,
                })
                .to_string(),
            )),
        }
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::SerialGlobal
    }
}
