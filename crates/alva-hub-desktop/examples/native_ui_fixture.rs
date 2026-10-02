// INPUT: An explicit, empty absolute directory and a deterministic engine double
// OUTPUT: Private Hub fixture data, two local Git worktrees and browser test pages
// POS: Reproducible native UI QA setup without account access or real engine execution.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest, ToolOutput,
};
use alva_hub_desktop::{run_persisted, HubStore, PermissionProfile, RunRequest};
use alva_kernel_abi::StreamEvent;
use async_trait::async_trait;
use futures_core::Stream;

struct FixtureEngine {
    events: Vec<RuntimeEvent>,
}

#[async_trait]
impl EngineRuntime for FixtureEngine {
    fn execute(
        &self,
        _: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        Ok(Box::pin(tokio_stream::iter(self.events.clone())))
    }
    async fn cancel(&self, _: &str) -> Result<(), RuntimeError> {
        Ok(())
    }
    async fn respond_permission(
        &self,
        _: &str,
        _: &str,
        _: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: true,
            resume: true,
            cancel: true,
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Hub UI fixture",
            "-c",
            "user.email=fixture@invalid.example",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture Git operation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::main]
async fn main() {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("pass an empty absolute fixture directory"),
    );
    assert!(
        root.is_absolute() && root.is_dir() && root.read_dir().unwrap().next().is_none(),
        "fixture directory must exist and be empty"
    );
    let main = root.join("workspace-main");
    let linked = root.join("workspace-linked");
    std::fs::create_dir(&main).unwrap();
    std::fs::create_dir(main.join("nested")).unwrap();
    std::fs::write(main.join("hello.txt"), "base\n").unwrap();
    std::fs::write(
        main.join("nested/中文.txt"),
        "中文与 emoji 🧭\n<strong>This is inert text</strong>\n",
    )
    .unwrap();
    git(&main, &["init", "-b", "main"]);
    git(&main, &["add", "hello.txt", "nested/中文.txt"]);
    git(&main, &["commit", "-m", "fixture baseline"]);
    git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().unwrap(),
            "-b",
            "qa-linked",
        ],
    );
    std::fs::write(main.join("hello.txt"), "base\nmain staged change\n").unwrap();
    git(&main, &["add", "hello.txt"]);
    std::fs::write(
        main.join("hello.txt"),
        "base\nmain staged change\nmain unstaged change\n",
    )
    .unwrap();
    std::fs::write(main.join("new-file.txt"), "main untracked\n").unwrap();
    std::fs::write(linked.join("hello.txt"), "base\nlinked change only\n").unwrap();

    let store = HubStore::open(root.join("data/alva-hub.db")).unwrap();
    let project = store
        .create_project(main.to_str().unwrap(), Some("Native QA"))
        .unwrap();
    for (index, workspace) in [&main, &linked].into_iter().enumerate() {
        let message = if index == 0 {
            "# Main workspace\n\nDurable assistant text 🧭.\n\n```rust\nlet answer = 42;\n```\n"
        } else {
            "# Linked worktree\n\nThis session has its own cwd and Git branch.\n"
        };
        let engine = Arc::new(FixtureEngine {
            events: vec![
                RuntimeEvent::SessionStarted {
                    session_id: format!("fixture-{index}"),
                    model: None,
                    tools: Vec::new(),
                },
                RuntimeEvent::MessageDelta {
                    id: "fixture-message".into(),
                    delta: StreamEvent::TextDelta {
                        text: message.into(),
                    },
                },
                RuntimeEvent::ToolStart {
                    id: "fixture-tool".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({ "path": "hello.txt" }),
                },
                RuntimeEvent::ToolEnd {
                    id: "fixture-tool".into(),
                    name: "read_file".into(),
                    result: ToolOutput::text("Full Unicode tool output 🧭\n".repeat(2500)),
                    duration_ms: Some(12),
                },
                RuntimeEvent::Completed {
                    session_id: format!("fixture-{index}"),
                    result: Some(message.into()),
                    usage: None,
                },
            ],
        });
        let outcome = run_persisted(
            &store,
            move |_, _, _| Ok(engine.clone() as Arc<dyn EngineRuntime>),
            |_| None,
            RunRequest {
                prompt: format!("Native QA {}", if index == 0 { "main" } else { "linked" }),
                workspace: workspace.to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::WorkspaceWrite),
            },
            |_| {},
        )
        .await
        .unwrap();
        store
            .set_session_project(&outcome.product_session_id, Some(&project.project_id))
            .unwrap();
        println!("fixture session: {}", outcome.product_session_id);
    }
    let pages = root.join("pages");
    std::fs::create_dir(&pages).unwrap();
    std::fs::write(pages.join("index.html"), r#"<!doctype html><meta charset="utf-8"><title>Hub browser QA · page one</title><h1>Native browser page one</h1><a href="two.html">Next page</a><p id="ipc">IPC not attempted</p><button onclick="probe()">Attempt Hub IPC</button><button onclick="window.open('two.html')">Attempt popup</button><a href="file:///tmp/hub-ui-nonexistent">Attempt file navigation</a><script>async function probe(){const status=document.getElementById('ipc');if(!window.__TAURI_INTERNALS__?.invoke){status.textContent='Hub IPC unavailable';return;}try{await window.__TAURI_INTERNALS__.invoke('list_hub_sessions');status.textContent='ERROR: Hub IPC allowed';}catch{status.textContent='Hub IPC blocked';}}</script>"#).unwrap();
    std::fs::write(pages.join("two.html"), "<!doctype html><meta charset=utf-8><title>Hub browser QA · page two</title><h1>Native browser page two</h1><a href=index.html>First page</a>").unwrap();
    println!("isolated app directory: {}", root.join("data").display());
    println!("browser pages: {}", pages.display());
}
