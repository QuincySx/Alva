// INPUT:  (none - no external dependencies)
// OUTPUT: pub trait Channel, pub struct LastValue, pub struct BinaryOperatorAggregate, pub struct EphemeralValue
// POS:    Typed state channels with version tracking for graph step communication.
/// Channel types for typed state management in graph execution.
///
/// Three channel implementations modeled after LangGraph:
/// - `LastValue<T>`: stores exactly one value; last update wins
/// - `BinaryOperatorAggregate<T>`: folds updates with a reducer function
/// - `EphemeralValue<T>`: like LastValue but `reset()` clears it

/// Base trait for typed state channels.
///
/// Each channel tracks a monotonic `version` that increments on every
/// successful update. The execution engine uses version numbers to detect
/// which channels have fresh data and decide which nodes to trigger.
pub trait Channel: Send + Sync {
    type Value: Clone + Send + Sync;
    type Update: Clone + Send + Sync;

    /// Get the current value, if any.
    fn get(&self) -> Option<Self::Value>;

    /// Apply a batch of updates from the current step.
    /// Returns `true` if the stored value changed.
    fn update(&mut self, values: Vec<Self::Update>) -> bool;

    /// Called between steps. Ephemeral channels clear here; persistent ones are no-ops.
    fn reset(&mut self);

    /// Monotonically increasing version — incremented on every successful update.
    /// Used by the execution engine to detect fresh data.
    fn version(&self) -> u64;
}

// ---------------------------------------------------------------------------
// LastValue<T>
// ---------------------------------------------------------------------------

/// Stores exactly one value. When multiple updates arrive in the same step,
/// the last one wins.
pub struct LastValue<T> {
    value: Option<T>,
    version: u64,
}

impl<T> LastValue<T> {
    pub fn new() -> Self {
        Self {
            value: None,
            version: 0,
        }
    }

    pub fn with_initial(value: T) -> Self {
        Self {
            value: Some(value),
            version: 0,
        }
    }
}

impl<T> Default for LastValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Send + Sync + PartialEq> Channel for LastValue<T> {
    type Value = T;
    type Update = T;

    fn get(&self) -> Option<T> {
        self.value.clone()
    }

    fn update(&mut self, values: Vec<T>) -> bool {
        if let Some(last) = values.into_iter().last() {
            let changed = self.value.as_ref() != Some(&last);
            self.value = Some(last);
            if changed {
                self.version += 1;
            }
            changed
        } else {
            false
        }
    }

    fn reset(&mut self) {
        // Persistent — do nothing
    }

    fn version(&self) -> u64 {
        self.version
    }
}

// ---------------------------------------------------------------------------
// BinaryOperatorAggregate<T>
// ---------------------------------------------------------------------------

/// Folds all updates in a step using a binary reducer `Fn(T, T) -> T`.
///
/// Example: accumulating a `Vec<i32>` with `|mut a, b| { a.extend(b); a }`.
pub struct BinaryOperatorAggregate<T> {
    value: Option<T>,
    operator: Box<dyn Fn(T, T) -> T + Send + Sync>,
    version: u64,
}

impl<T> BinaryOperatorAggregate<T> {
    pub fn new(operator: impl Fn(T, T) -> T + Send + Sync + 'static) -> Self {
        Self {
            value: None,
            operator: Box::new(operator),
            version: 0,
        }
    }

    pub fn with_initial(
        value: T,
        operator: impl Fn(T, T) -> T + Send + Sync + 'static,
    ) -> Self {
        Self {
            value: Some(value),
            operator: Box::new(operator),
            version: 0,
        }
    }
}

impl<T: Clone + Send + Sync + PartialEq> Channel for BinaryOperatorAggregate<T> {
    type Value = T;
    type Update = T;

    fn get(&self) -> Option<T> {
        self.value.clone()
    }

    fn update(&mut self, values: Vec<T>) -> bool {
        if values.is_empty() {
            return false;
        }

        let mut iter = values.into_iter();

        let folded = if let Some(current) = self.value.take() {
            iter.fold(current, &*self.operator)
        } else {
            let first = iter.next().unwrap();
            iter.fold(first, &*self.operator)
        };

        let changed = self.value.as_ref() != Some(&folded);
        self.value = Some(folded);
        if changed {
            self.version += 1;
        }
        changed
    }

    fn reset(&mut self) {
        // Persistent — do nothing
    }

    fn version(&self) -> u64 {
        self.version
    }
}

// ---------------------------------------------------------------------------
// EphemeralValue<T>
// ---------------------------------------------------------------------------

/// Like `LastValue` but `reset()` clears the stored value.
/// Useful for input/output channels that should not carry state across steps.
pub struct EphemeralValue<T> {
    value: Option<T>,
    version: u64,
}

impl<T> EphemeralValue<T> {
    pub fn new() -> Self {
        Self {
            value: None,
            version: 0,
        }
    }
}

impl<T> Default for EphemeralValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Send + Sync + PartialEq> Channel for EphemeralValue<T> {
    type Value = T;
    type Update = T;

    fn get(&self) -> Option<T> {
        self.value.clone()
    }

    fn update(&mut self, values: Vec<T>) -> bool {
        if let Some(last) = values.into_iter().last() {
            let changed = self.value.as_ref() != Some(&last);
            self.value = Some(last);
            if changed {
                self.version += 1;
            }
            changed
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.value = None;
    }

    fn version(&self) -> u64 {
        self.version
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- LastValue tests --

    #[test]
    fn last_value_empty_initially() {
        let ch: LastValue<i32> = LastValue::new();
        assert_eq!(ch.get(), None);
        assert_eq!(ch.version(), 0);
    }

    #[test]
    fn last_value_single_update() {
        let mut ch: LastValue<i32> = LastValue::new();
        let changed = ch.update(vec![42]);
        assert!(changed);
        assert_eq!(ch.get(), Some(42));
        assert_eq!(ch.version(), 1);
    }

    #[test]
    fn last_value_multiple_updates_last_wins() {
        let mut ch: LastValue<i32> = LastValue::new();
        let changed = ch.update(vec![1, 2, 3]);
        assert!(changed);
        assert_eq!(ch.get(), Some(3));
        assert_eq!(ch.version(), 1);
    }

    #[test]
    fn last_value_no_change_returns_false() {
        let mut ch: LastValue<i32> = LastValue::new();
        ch.update(vec![5]);
        let changed = ch.update(vec![5]);
        assert!(!changed);
        assert_eq!(ch.get(), Some(5));
        assert_eq!(ch.version(), 1); // no increment on same value
    }

    #[test]
    fn last_value_reset_is_noop() {
        let mut ch: LastValue<i32> = LastValue::new();
        ch.update(vec![10]);
        ch.reset();
        assert_eq!(ch.get(), Some(10));
    }

    #[test]
    fn last_value_empty_update_returns_false() {
        let mut ch: LastValue<i32> = LastValue::new();
        let changed = ch.update(vec![]);
        assert!(!changed);
        assert_eq!(ch.get(), None);
    }

    #[test]
    fn last_value_version_increments() {
        let mut ch: LastValue<i32> = LastValue::new();
        assert_eq!(ch.version(), 0);
        ch.update(vec![1]);
        assert_eq!(ch.version(), 1);
        ch.update(vec![2]);
        assert_eq!(ch.version(), 2);
        ch.update(vec![2]); // same value — no increment
        assert_eq!(ch.version(), 2);
    }

    // -- BinaryOperatorAggregate tests --

    #[test]
    fn binary_op_fold_addition() {
        let mut ch = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        let changed = ch.update(vec![1, 2, 3]);
        assert!(changed);
        assert_eq!(ch.get(), Some(6));
        assert_eq!(ch.version(), 1);
    }

    #[test]
    fn binary_op_accumulates_across_updates() {
        let mut ch = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        ch.update(vec![10]);
        assert_eq!(ch.get(), Some(10));
        assert_eq!(ch.version(), 1);
        ch.update(vec![5]);
        assert_eq!(ch.get(), Some(15));
        assert_eq!(ch.version(), 2);
        ch.update(vec![3, 2]);
        assert_eq!(ch.get(), Some(20));
        assert_eq!(ch.version(), 3);
    }

    #[test]
    fn binary_op_with_initial_value() {
        let mut ch = BinaryOperatorAggregate::with_initial(100, |a: i32, b: i32| a + b);
        ch.update(vec![1]);
        assert_eq!(ch.get(), Some(101));
    }

    #[test]
    fn binary_op_vec_accumulation() {
        let mut ch = BinaryOperatorAggregate::new(|mut a: Vec<i32>, b: Vec<i32>| {
            a.extend(b);
            a
        });
        ch.update(vec![vec![1, 2]]);
        assert_eq!(ch.get(), Some(vec![1, 2]));
        ch.update(vec![vec![3], vec![4, 5]]);
        assert_eq!(ch.get(), Some(vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn binary_op_empty_update_returns_false() {
        let mut ch = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        let changed = ch.update(vec![]);
        assert!(!changed);
        assert_eq!(ch.get(), None);
    }

    #[test]
    fn binary_op_reset_is_noop() {
        let mut ch = BinaryOperatorAggregate::new(|a: i32, b: i32| a + b);
        ch.update(vec![42]);
        ch.reset();
        assert_eq!(ch.get(), Some(42));
    }

    // -- EphemeralValue tests --

    #[test]
    fn ephemeral_empty_initially() {
        let ch: EphemeralValue<i32> = EphemeralValue::new();
        assert_eq!(ch.get(), None);
        assert_eq!(ch.version(), 0);
    }

    #[test]
    fn ephemeral_update_and_get() {
        let mut ch: EphemeralValue<String> = EphemeralValue::new();
        let changed = ch.update(vec!["hello".to_string()]);
        assert!(changed);
        assert_eq!(ch.get(), Some("hello".to_string()));
        assert_eq!(ch.version(), 1);
    }

    #[test]
    fn ephemeral_reset_clears_value() {
        let mut ch: EphemeralValue<i32> = EphemeralValue::new();
        ch.update(vec![99]);
        assert_eq!(ch.get(), Some(99));
        ch.reset();
        assert_eq!(ch.get(), None);
    }

    #[test]
    fn ephemeral_multiple_updates_last_wins() {
        let mut ch: EphemeralValue<i32> = EphemeralValue::new();
        ch.update(vec![1, 2, 3]);
        assert_eq!(ch.get(), Some(3));
    }
}
