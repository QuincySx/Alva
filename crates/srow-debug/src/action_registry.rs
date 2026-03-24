use parking_lot::RwLock;
use std::collections::HashMap;
use serde_json::Value;

/// Type-erased action closure: receives method name + args JSON, returns result or error.
pub type ActionFn = Box<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;

/// Type-erased state closure: returns current component state as JSON.
pub type StateFn = Box<dyn Fn() -> Option<Value> + Send + Sync>;

/// A registered view with type-erased action dispatch and state reading.
pub struct RegisteredView {
    pub action_fn: ActionFn,
    pub state_fn: StateFn,
    pub methods: Vec<String>,
}

/// Thread-safe registry mapping view IDs to type-erased action/state closures.
pub struct ActionRegistry {
    views: RwLock<HashMap<String, RegisteredView>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            views: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, id: &str, view: RegisteredView) {
        self.views.write().insert(id.to_string(), view);
    }

    pub fn unregister(&self, id: &str) {
        self.views.write().remove(id);
    }

    /// Returns (view_id, methods) for all registered views.
    pub fn list_views(&self) -> Vec<(String, Vec<String>)> {
        self.views
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.methods.clone()))
            .collect()
    }

    /// Dispatch an action to a registered view.
    pub fn dispatch(&self, target: &str, method: &str, args: Value) -> Result<Value, String> {
        let views = self.views.read();
        let view = views
            .get(target)
            .ok_or_else(|| format!("target '{}' not registered or dropped", target))?;
        if !view.methods.contains(&method.to_string()) {
            return Err(format!("method '{}' not found on '{}'", method, target));
        }
        (view.action_fn)(method, args)
    }

    /// Read state from a registered view.
    pub fn get_state(&self, target: &str) -> Result<Value, String> {
        let views = self.views.read();
        let view = views
            .get(target)
            .ok_or_else(|| format!("target '{}' not registered or dropped", target))?;
        (view.state_fn)().ok_or_else(|| format!("entity '{}' has been dropped", target))
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_register_and_list_views() {
        let registry = ActionRegistry::new();
        registry.register(
            "test_view",
            RegisteredView {
                action_fn: Box::new(|_method, _args| Ok(serde_json::Value::Null)),
                state_fn: Box::new(|| Some(json!({"count": 0}))),
                methods: vec!["do_thing".into()],
            },
        );
        let views = registry.list_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].0, "test_view");
        assert_eq!(views[0].1, vec!["do_thing".to_string()]);
    }

    #[test]
    fn test_unregister_view() {
        let registry = ActionRegistry::new();
        registry.register(
            "v1",
            RegisteredView {
                action_fn: Box::new(|_, _| Ok(serde_json::Value::Null)),
                state_fn: Box::new(|| None),
                methods: vec![],
            },
        );
        assert_eq!(registry.list_views().len(), 1);
        registry.unregister("v1");
        assert_eq!(registry.list_views().len(), 0);
    }

    #[test]
    fn test_dispatch_success() {
        let registry = ActionRegistry::new();
        registry.register(
            "counter",
            RegisteredView {
                action_fn: Box::new(|method, _args| match method {
                    "increment" => Ok(json!({"new_value": 1})),
                    _ => Err(format!("unknown method: {method}")),
                }),
                state_fn: Box::new(|| Some(json!({"value": 0}))),
                methods: vec!["increment".into()],
            },
        );

        let result = registry.dispatch("counter", "increment", json!({}));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), json!({"new_value": 1}));
    }

    #[test]
    fn test_dispatch_unknown_target() {
        let registry = ActionRegistry::new();
        let result = registry.dispatch("nonexistent", "foo", json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not registered"));
    }

    #[test]
    fn test_dispatch_unknown_method() {
        let registry = ActionRegistry::new();
        registry.register(
            "view",
            RegisteredView {
                action_fn: Box::new(|_, _| Ok(json!(null))),
                state_fn: Box::new(|| None),
                methods: vec!["known".into()],
            },
        );
        let result = registry.dispatch("view", "unknown", json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_get_state() {
        let registry = ActionRegistry::new();
        registry.register(
            "panel",
            RegisteredView {
                action_fn: Box::new(|_, _| Ok(json!(null))),
                state_fn: Box::new(|| Some(json!({"messages": 5, "loading": false}))),
                methods: vec![],
            },
        );

        let state = registry.get_state("panel");
        assert!(state.is_ok());
        assert_eq!(state.unwrap()["messages"], 5);
    }

    #[test]
    fn test_get_state_unknown_target() {
        let registry = ActionRegistry::new();
        let state = registry.get_state("nope");
        assert!(state.is_err());
    }
}
