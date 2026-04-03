// INPUT:  types, engine, parallel submodules
// OUTPUT: pub re-exports of CompiledGraph, GraphEvent, InvokeConfig
// POS:    Module root — declares submodules and re-exports the public API.

mod engine;
mod parallel;
mod types;

pub use types::{CompiledGraph, GraphEvent, InvokeConfig};

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::mpsc;

    use crate::checkpoint::CheckpointSaver;
    use crate::graph::{NodeResult, SendTo, StateGraph, END};

    use super::*;

    #[tokio::test]
    async fn simple_linear_graph() {
        let mut graph = StateGraph::new();

        graph.add_node("step1", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["step1"] = json!(true);
                s
            })
        });

        graph.add_node("step2", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["step2"] = json!(true);
                s
            })
        });

        graph.set_entry_point("step1");
        graph.add_edge("step1", "step2");
        graph.add_edge("step2", END);

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();

        assert_eq!(result["step1"], true);
        assert_eq!(result["step2"], true);
    }

    #[tokio::test]
    async fn conditional_routing() {
        let mut graph = StateGraph::new();

        graph.add_node("router_node", |state| Box::pin(async { state }));
        graph.add_node("path_a", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["path"] = "a".into();
                s
            })
        });
        graph.add_node("path_b", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["path"] = "b".into();
                s
            })
        });

        graph.set_entry_point("router_node");
        graph.add_conditional_edge("router_node", |state| {
            if state
                .get("go_a")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                "path_a".to_string()
            } else {
                "path_b".to_string()
            }
        });
        graph.add_edge("path_a", END);
        graph.add_edge("path_b", END);

        let compiled = graph.compile().unwrap();

        let result = compiled.invoke(json!({"go_a": true})).await.unwrap();
        assert_eq!(result["path"], "a");

        let result = compiled.invoke(json!({"go_a": false})).await.unwrap();
        assert_eq!(result["path"], "b");
    }

    #[tokio::test]
    async fn single_node_no_explicit_edge_defaults_to_end() {
        let mut graph = StateGraph::new();
        graph.add_node("only", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["done"] = json!(true);
                s
            })
        });
        graph.set_entry_point("only");

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();
        assert_eq!(result["done"], true);
    }

    #[test]
    fn compile_fails_without_entry_point() {
        let graph = StateGraph::<serde_json::Value>::new();
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Entry point not set"));
    }

    #[test]
    fn compile_fails_with_invalid_entry_point() {
        let mut graph = StateGraph::<serde_json::Value>::new();
        graph.set_entry_point("nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_target() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s: serde_json::Value| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("a", "nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge target"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_source() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s: serde_json::Value| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("nonexistent", "a");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge source"));
    }

    #[tokio::test]
    async fn parallel_fan_out_execution() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let counter = Arc::new(AtomicU32::new(0));
        let mut graph = StateGraph::<serde_json::Value>::new();

        graph.add_node("entry", |s| Box::pin(async { s }));

        let c1 = counter.clone();
        graph.add_node("parallel_a", move |s: serde_json::Value| {
            let c = c1.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                let mut s = s;
                s["a"] = serde_json::json!(true);
                s
            })
        });

        let c2 = counter.clone();
        graph.add_node("parallel_b", move |s: serde_json::Value| {
            let c = c2.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                let mut s = s;
                s["b"] = serde_json::json!(true);
                s
            })
        });

        graph.add_node("merge", |s| Box::pin(async { s }));

        graph.set_entry_point("entry");
        graph.add_edge("entry", "parallel_a");
        graph.add_edge("entry", "parallel_b");
        graph.add_edge("parallel_a", "merge");
        graph.add_edge("parallel_b", "merge");
        graph.add_edge("merge", END);

        let compiled = graph.compile().unwrap();
        let _result = compiled.invoke(serde_json::json!({})).await.unwrap();

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "both parallel nodes should execute"
        );
    }

    #[tokio::test]
    async fn parallel_fan_out_with_merge() {
        let mut graph = StateGraph::<serde_json::Value>::new();

        graph.add_node("entry", |s| Box::pin(async { s }));
        graph.add_node("add_a", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["a"] = serde_json::json!(true);
                s
            })
        });
        graph.add_node("add_b", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["b"] = serde_json::json!(true);
                s
            })
        });
        graph.add_node("final", |s| Box::pin(async { s }));

        graph.set_entry_point("entry");
        graph.add_edge("entry", "add_a");
        graph.add_edge("entry", "add_b");
        graph.add_edge("add_a", "final");
        graph.add_edge("add_b", "final");
        graph.add_edge("final", END);

        graph.set_merge(|base, outputs| {
            let mut merged = base;
            for output in outputs {
                if let (Some(m), Some(o)) = (merged.as_object_mut(), output.as_object()) {
                    for (k, v) in o {
                        m.insert(k.clone(), v.clone());
                    }
                }
            }
            merged
        });

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(serde_json::json!({})).await.unwrap();

        assert_eq!(result["a"], true);
        assert_eq!(result["b"], true);
    }

    #[tokio::test]
    async fn three_node_chain() {
        let mut graph = StateGraph::new();

        graph.add_node("a", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["order"] = json!("a");
                s
            })
        });
        graph.add_node("b", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let prev = s["order"].as_str().unwrap_or("").to_string();
                s["order"] = json!(format!("{},b", prev));
                s
            })
        });
        graph.add_node("c", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let prev = s["order"].as_str().unwrap_or("").to_string();
                s["order"] = json!(format!("{},c", prev));
                s
            })
        });

        graph.set_entry_point("a");
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", END);

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();
        assert_eq!(result["order"], "a,b,c");
    }

    // -- New tests for Send, events, checkpoint --

    #[tokio::test]
    async fn send_dynamic_routing() {
        let mut graph = StateGraph::<serde_json::Value>::new();

        // Router node uses Send to fan out dynamically
        graph.add_routing_node("router", |state: serde_json::Value| {
            Box::pin(async move {
                let items = state["items"].as_array().cloned().unwrap_or_default();
                let sends: Vec<SendTo<serde_json::Value>> = items
                    .into_iter()
                    .map(|item| SendTo {
                        node: "worker".into(),
                        state: json!({ "item": item }),
                    })
                    .collect();
                NodeResult::Sends(sends)
            })
        });

        graph.add_node("worker", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let item = s["item"].clone();
                s["processed"] = json!(format!("done:{}", item));
                s
            })
        });

        graph.set_entry_point("router");
        graph.add_edge("worker", END);

        graph.set_merge(|_base, outputs| {
            let results: Vec<String> = outputs
                .iter()
                .filter_map(|o| o["processed"].as_str().map(String::from))
                .collect();
            json!({ "results": results })
        });

        let compiled = graph.compile().unwrap();
        let result = compiled
            .invoke(json!({ "items": [1, 2, 3] }))
            .await
            .unwrap();

        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn invoke_with_events() {
        let mut graph = StateGraph::new();
        graph.add_node("step1", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["done"] = json!(true);
                s
            })
        });
        graph.set_entry_point("step1");
        graph.add_edge("step1", END);

        let compiled = graph.compile().unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = InvokeConfig::default().with_events(tx);

        let _result = compiled.invoke_with_config(json!({}), config).await.unwrap();

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        assert!(events.iter().any(|e| matches!(e, GraphEvent::SuperstepStart { step: 1 })));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::NodeStart { node, .. } if node == "step1")));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::NodeEnd { node, .. } if node == "step1")));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn invoke_with_checkpoint() {
        let mut graph = StateGraph::new();
        graph.add_node("s1", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["s1"] = json!(true);
                s
            })
        });
        graph.add_node("s2", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["s2"] = json!(true);
                s
            })
        });
        graph.set_entry_point("s1");
        graph.add_edge("s1", "s2");
        graph.add_edge("s2", END);

        let compiled = graph.compile().unwrap();

        let saver = Arc::new(crate::checkpoint::InMemoryCheckpointSaver::new());
        let config = InvokeConfig::default()
            .with_checkpoint(saver.clone())
            .with_checkpoint_id("test");

        let _result = compiled.invoke_with_config(json!({}), config).await.unwrap();

        // Should have 2 checkpoints (one per superstep)
        let ids = saver.list().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|id| id == "test-step-1"));
        assert!(ids.iter().any(|id| id == "test-step-2"));

        // Verify checkpoint content
        let cp2 = saver.load("test-step-2").await.unwrap().unwrap();
        assert_eq!(cp2["s1"], true);
        assert_eq!(cp2["s2"], true);
    }
}
