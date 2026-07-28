// INPUT:  process argv/env, alva_hub_core registry, Alva BaseAgent adapter, provider configuration
// OUTPUT: alva-hub binary for a one-shot Alva Hub session
// POS:    Minimal CLI vertical slice proving Hub owns engine dispatch while native Alva remains an adapter.

use std::sync::Arc;

use alva_engine_adapter_alva::{
    AlvaAdapter, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig,
};
use alva_engine_runtime::{RuntimeEvent, RuntimeRequest};
use alva_hub_core::{HubRequest, HubRuntime};
use tokio_stream::StreamExt;

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to start Tokio runtime");
    let exit_code = runtime.block_on(run());
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

async fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        return 0;
    }
    let prompt = args.join(" ");
    if prompt.is_empty() {
        print_usage();
        return 2;
    }

    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("alva-hub: cannot determine workspace: {error}");
            return 1;
        }
    };
    let config = match alva_llm_provider::ProviderConfig::load(&workspace) {
        Ok(config) if !config.api_key.is_empty() => config,
        Ok(_) => {
            eprintln!("alva-hub: no provider API key configured (set ALVA_API_KEY)");
            return 2;
        }
        Err(error) => {
            eprintln!("alva-hub: cannot load provider configuration: {error}");
            return 1;
        }
    };
    let provider_kind = config.kind.clone();
    let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
    let factory =
        DefaultBaseAgentFactory::new(DefaultBaseAgentFactoryConfig::new(model, workspace));
    let hub = HubRuntime::new();
    if let Err(error) = hub.register_engine("alva", Arc::new(AlvaAdapter::new(Arc::new(factory)))) {
        eprintln!("alva-hub: {error}");
        return 1;
    }

    let mut events = match hub.execute(HubRequest::new("alva", RuntimeRequest::new(prompt))) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            return 1;
        }
    };
    let mut failed = false;
    while let Some(hub_event) = events.next().await {
        match hub_event.event {
            RuntimeEvent::Message { content, .. } => {
                for block in content {
                    if let Some(text) = block.as_text() {
                        print!("{text}");
                    }
                }
            }
            RuntimeEvent::Error { message, .. } => {
                failed = true;
                eprintln!("alva-hub: {message}");
            }
            RuntimeEvent::Completed { .. } => println!(),
            _ => {}
        }
    }
    if failed {
        1
    } else {
        0
    }
}

fn print_usage() {
    println!("Usage: alva-hub <prompt>\n\nRuns one Alva Hub product session using the built-in Alva engine.");
}
