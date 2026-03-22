use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::inspect::{InspectNode, Inspectable};

pub struct ViewEntry {
    pub id: String,
    pub type_name: String,
    pub parent_id: Option<String>,
    pub snapshot_fn: Box<dyn Fn() -> InspectNode + Send + Sync>,
}

pub struct ViewRegistry {
    entries: RwLock<Vec<ViewEntry>>,
}

impl ViewRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
        })
    }

    pub fn register(&self, entry: ViewEntry) {
        self.entries.write().push(entry);
    }

    pub fn unregister(&self, id: &str) {
        self.entries.write().retain(|e| e.id != id);
    }

    fn build_tree(&self) -> InspectNode {
        let entries = self.entries.read();
        let snapshots: Vec<(Option<String>, InspectNode)> = entries
            .iter()
            .map(|e| (e.parent_id.clone(), (e.snapshot_fn)()))
            .collect();
        build_tree_from_flat(snapshots)
    }
}

fn build_tree_from_flat(items: Vec<(Option<String>, InspectNode)>) -> InspectNode {
    if items.is_empty() {
        return InspectNode {
            id: "empty".to_string(),
            type_name: "Empty".to_string(),
            bounds: None,
            properties: HashMap::new(),
            children: vec![],
        };
    }

    let mut children_map: HashMap<String, Vec<InspectNode>> = HashMap::new();
    let mut roots = Vec::new();

    for (parent_id, node) in items {
        match parent_id {
            Some(pid) => children_map.entry(pid).or_default().push(node),
            None => roots.push(node),
        }
    }

    fn attach_children(
        node: &mut InspectNode,
        children_map: &mut HashMap<String, Vec<InspectNode>>,
    ) {
        if let Some(children) = children_map.remove(&node.id) {
            for mut child in children {
                attach_children(&mut child, children_map);
                node.children.push(child);
            }
        }
    }

    if roots.len() == 1 {
        let mut root = roots.remove(0);
        attach_children(&mut root, &mut children_map);
        root
    } else {
        let mut root = InspectNode {
            id: "root".to_string(),
            type_name: "Root".to_string(),
            bounds: None,
            properties: HashMap::new(),
            children: roots,
        };
        for child in &mut root.children {
            attach_children(child, &mut children_map);
        }
        root
    }
}

pub struct GpuiInspector {
    registry: Arc<ViewRegistry>,
}

impl GpuiInspector {
    pub fn new(registry: Arc<ViewRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Arc<ViewRegistry> {
        &self.registry
    }
}

impl Inspectable for GpuiInspector {
    fn inspect(&self) -> InspectNode {
        self.registry.build_tree()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builds_tree() {
        let registry = ViewRegistry::new();
        registry.register(ViewEntry {
            id: "root".to_string(),
            type_name: "RootView".to_string(),
            parent_id: None,
            snapshot_fn: Box::new(|| InspectNode {
                id: "root".to_string(),
                type_name: "RootView".to_string(),
                bounds: None,
                properties: HashMap::new(),
                children: vec![],
            }),
        });
        registry.register(ViewEntry {
            id: "panel".to_string(),
            type_name: "ChatPanel".to_string(),
            parent_id: Some("root".to_string()),
            snapshot_fn: Box::new(|| InspectNode {
                id: "panel".to_string(),
                type_name: "ChatPanel".to_string(),
                bounds: None,
                properties: {
                    let mut m = HashMap::new();
                    m.insert("msg_count".to_string(), serde_json::json!(5));
                    m
                },
                children: vec![],
            }),
        });

        let inspector = GpuiInspector::new(registry);
        let tree = inspector.inspect();
        assert_eq!(tree.id, "root");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].type_name, "ChatPanel");
        assert_eq!(tree.children[0].properties["msg_count"], 5);
    }

    #[test]
    fn unregister_removes_view() {
        let registry = ViewRegistry::new();
        registry.register(ViewEntry {
            id: "temp".to_string(),
            type_name: "TempView".to_string(),
            parent_id: None,
            snapshot_fn: Box::new(|| InspectNode {
                id: "temp".to_string(),
                type_name: "TempView".to_string(),
                bounds: None,
                properties: HashMap::new(),
                children: vec![],
            }),
        });

        let inspector = GpuiInspector::new(registry.clone());
        assert_eq!(inspector.inspect().type_name, "TempView");

        registry.unregister("temp");
        assert_eq!(inspector.inspect().type_name, "Empty");
    }
}
