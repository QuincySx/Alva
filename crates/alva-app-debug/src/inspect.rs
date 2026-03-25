use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct InspectNode {
    pub id: String,
    pub type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    pub properties: HashMap<String, serde_json::Value>,
    pub children: Vec<InspectNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub trait Inspectable: Send + Sync {
    fn inspect(&self) -> InspectNode;
}

#[cfg(debug_assertions)]
pub trait DebugInspect {
    fn debug_properties(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_node_serializes_to_json() {
        let node = InspectNode {
            id: "root".to_string(),
            type_name: "RootView".to_string(),
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }),
            properties: HashMap::new(),
            children: vec![InspectNode {
                id: "child".to_string(),
                type_name: "Panel".to_string(),
                bounds: None,
                properties: {
                    let mut m = HashMap::new();
                    m.insert("count".to_string(), serde_json::json!(5));
                    m
                },
                children: vec![],
            }],
        };

        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["id"], "root");
        assert_eq!(json["children"][0]["type_name"], "Panel");
        assert_eq!(json["children"][0]["properties"]["count"], 5);
        assert!(json["children"][0].get("bounds").is_none());
    }
}
