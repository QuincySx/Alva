// INPUT:  alva_app_core, tokio, std::io
// OUTPUT: pub fn main
// POS:    Minimal CLI binary for testing the Agent engine — reads prompt from stdin, runs agent, prints events.
//! alva-cli — Simple CLI to test the Agent engine
//!
//! Usage: cargo run -p alva-app-core --bin alva-cli
//!
//! Reads a single prompt from stdin (or first CLI argument), runs the
//! BaseAgent, and prints events to stdout.

use std::io::{self, BufRead};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    rt.block_on(async {
        // Get prompt from CLI arg or stdin
        let prompt = std::env::args().nth(1).unwrap_or_else(|| {
            eprintln!("Enter prompt (Ctrl+D to finish):");
            let stdin = io::stdin();
            stdin
                .lock()
                .lines()
                .map_while(Result::ok)
                .collect::<Vec<_>>()
                .join("\n")
        });

        if prompt.trim().is_empty() {
            eprintln!("Error: empty prompt");
            std::process::exit(1);
        }

        eprintln!("Prompt: {}", prompt);
        eprintln!("---");
        eprintln!("Note: BaseAgent requires a configured LanguageModel provider.");
        eprintln!("This CLI is a skeleton — connect a real provider to use it.");

        // TODO: Build a BaseAgent with a real provider and run:
        //   let agent = BaseAgentBuilder::new()
        //       .with_workspace(".")
        //       .build()
        //       .expect("failed to build agent");
        //   let mut rx = agent.prompt_text(&prompt);
        //   while let Some(event) = rx.recv().await {
        //       println!("{:?}", event);
        //   }
    });
}
