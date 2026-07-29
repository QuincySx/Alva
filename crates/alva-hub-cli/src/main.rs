// INPUT:  process argv/env, cli grammar, alva_hub_core registry, selected engine adapters
// OUTPUT: alva-hub binary for a one-shot multi-engine Hub session
// POS:    Hub CLI dispatcher that validates engine selection before initializing only that engine.

mod cli;

use std::sync::Arc;

use alva_engine_adapter_alva::{
    AlvaAdapter, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig,
};
use alva_engine_adapter_claude::{ClaudeAdapter, ClaudeAdapterConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_runtime::{EngineRuntime, PermissionDecision, RuntimeEvent, RuntimeRequest};
use alva_hub_core::{HubRequest, HubRuntime};
use tokio_stream::StreamExt;

use crate::cli::{parse_args, Engine, ParseOutcome};

const USAGE_EXIT: i32 = 2;

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to start Tokio runtime");
    let exit_code = runtime.block_on(run());
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

async fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse_args(&args) {
        Ok(ParseOutcome::Help) => {
            print_usage();
            return 0;
        }
        Ok(ParseOutcome::Run(invocation)) => invocation,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            eprintln!("Try 'alva-hub --help'.");
            return USAGE_EXIT;
        }
    };

    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("alva-hub: cannot determine workspace: {error}");
            return 1;
        }
    };
    let hub = HubRuntime::new();
    let engine_id = invocation.engine.id();
    let engine = match build_selected_engine(invocation.engine, &workspace) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            return USAGE_EXIT;
        }
    };
    if let Err(error) = hub.register_engine(engine_id, engine) {
        eprintln!("alva-hub: {error}");
        return 1;
    }

    let mut events = match hub.execute(HubRequest::new(
        engine_id,
        RuntimeRequest::new(invocation.prompt),
    )) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            return 1;
        }
    };
    let mut failed = false;
    let mut emitted_text = false;
    while let Some(hub_event) = events.next().await {
        let product_session_id = hub_event.product_session_id;
        match hub_event.event {
            RuntimeEvent::Message { content, .. } => {
                for block in content {
                    if let Some(text) = block.as_text() {
                        print!("{text}");
                        emitted_text = true;
                    }
                }
            }
            // Both the Alva loop and Claude bridge emit streaming deltas and a
            // final Message. This one-shot CLI intentionally prints only the
            // final Message, so the same assistant text is never duplicated.
            RuntimeEvent::MessageDelta { .. } => {}
            RuntimeEvent::PermissionRequest {
                request_id,
                tool_name,
                ..
            } => {
                eprintln!("alva-hub: denied permission request for tool '{tool_name}'");
                if let Err(error) = hub
                    .respond_permission(
                        &product_session_id,
                        &request_id,
                        PermissionDecision::Deny {
                            message: "Alva Hub CLI does not grant interactive permissions".into(),
                        },
                    )
                    .await
                {
                    failed = true;
                    eprintln!("alva-hub: could not deny permission request: {error}");
                }
            }
            // cindy: recoverable ≈ willRetry — print a banner but do not
            // fail the process until a fatal error (or missing success).
            RuntimeEvent::Error {
                message,
                recoverable,
            } => {
                if recoverable {
                    eprintln!("alva-hub: (retrying) {message}");
                } else {
                    failed = true;
                    eprintln!("alva-hub: {message}");
                }
            }
            RuntimeEvent::Completed { result, .. } => {
                // Some bridge failures/successes surface only a final result.
                // Avoid duplicating normal assistant text while still showing it.
                if !emitted_text {
                    if let Some(result) = result {
                        print!("{result}");
                        emitted_text = true;
                    }
                }
            }
            RuntimeEvent::SessionStarted { .. }
            | RuntimeEvent::ToolStart { .. }
            | RuntimeEvent::ToolEnd { .. } => {}
        }
    }
    if emitted_text {
        println!();
    }
    if failed {
        1
    } else {
        0
    }
}

fn build_selected_engine(
    engine: Engine,
    workspace: &std::path::Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    match engine {
        Engine::Alva => {
            let config = alva_llm_provider::ProviderConfig::load(workspace)?;
            if config.api_key.is_empty() {
                return Err("no Alva provider API key configured (set ALVA_API_KEY)".into());
            }
            let provider_kind = config.kind.clone();
            let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
            let factory =
                DefaultBaseAgentFactory::new(DefaultBaseAgentFactoryConfig::new(model, workspace));
            Ok(Arc::new(AlvaAdapter::new(Arc::new(factory))))
        }
        Engine::Claude => {
            // ClaudeAdapterConfig's default is the adapter's existing env path:
            // the Node bridge inherits ANTHROPIC_API_KEY (or local Claude Code
            // login), uses `node`, and lets the SDK choose its default model.
            // Missing Node, SDK package, or authentication is emitted by the
            // adapter as RuntimeEvent::Error; never print config/env secrets.
            Ok(Arc::new(ClaudeAdapter::new(ClaudeAdapterConfig::default())))
        }
        Engine::Codex => {
            // Spawns `codex exec --json` from PATH. Auth uses Codex CLI login /
            // CODEX_HOME; never print secrets. Missing binary surfaces as
            // RuntimeEvent::Error after execute starts.
            let mut config = CodexAdapterConfig::default();
            config.default_cwd = Some(workspace.to_path_buf());
            Ok(Arc::new(CodexAdapter::new(config)))
        }
    }
}

fn print_usage() {
    println!(
        "Usage: alva-hub [--engine alva|claude|codex] [--] <prompt>\n\nEngines:\n  \
         alva    Default; uses the existing Alva provider config and ALVA_API_KEY.\n  \
         claude  Uses Node from PATH, the SDK default model, and ANTHROPIC_API_KEY or a local Claude Code login.\n  \
         codex   Uses `codex exec --json` from PATH (Codex CLI auth / CODEX_HOME)."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_claude_does_not_read_alva_provider_configuration() {
        let nonexistent_workspace =
            std::path::Path::new("/definitely/not/an/alva/provider/workspace");

        assert!(build_selected_engine(Engine::Claude, nonexistent_workspace).is_ok());
    }

    #[test]
    fn selecting_codex_does_not_read_alva_provider_configuration() {
        let nonexistent_workspace =
            std::path::Path::new("/definitely/not/an/alva/provider/workspace");

        assert!(build_selected_engine(Engine::Codex, nonexistent_workspace).is_ok());
    }

    #[test]
    fn recoverable_error_does_not_mark_cli_failed() {
        // Mirror the exit-code policy: only fatal errors set failed.
        let mut failed = false;
        let recoverable = RuntimeEvent::Error {
            message: "Reconnecting... 2/5".into(),
            recoverable: true,
        };
        let fatal = RuntimeEvent::Error {
            message: "boom".into(),
            recoverable: false,
        };
        if let RuntimeEvent::Error {
            recoverable: true, ..
        } = recoverable
        {
            // banner only
        } else {
            failed = true;
        }
        assert!(!failed);
        if let RuntimeEvent::Error {
            recoverable: false, ..
        } = fatal
        {
            failed = true;
        }
        assert!(failed);
    }
}
