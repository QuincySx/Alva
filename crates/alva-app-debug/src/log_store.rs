// INPUT:  serde::Serialize, std::collections::HashMap, serde_json::Value
// OUTPUT: pub struct LogRecord, pub struct LogQuery, pub struct LogQueryResponse, pub struct LogStore
// POS:    Ring-buffer log store with filtered query support for level, module, cursor, and keyword.
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    pub seq: u64,
    pub timestamp: i64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: HashMap<String, serde_json::Value>,
    pub span_stack: Vec<String>,
}

#[derive(Debug, Default)]
pub struct LogQuery {
    pub level: Option<String>,
    pub module: Option<String>,
    pub since: Option<i64>,
    pub cursor: Option<u64>,
    pub keyword: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct LogQueryResponse {
    pub total_matches: usize,
    pub records: Vec<LogRecord>,
}

pub struct LogStore {
    buffer: Vec<Option<LogRecord>>,
    capacity: usize,
    write_pos: usize,
    count: usize,
    seq_counter: u64,
}

impl LogStore {
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize_with(capacity, || None);
        Self {
            buffer,
            capacity,
            write_pos: 0,
            count: 0,
            seq_counter: 0,
        }
    }

    pub fn push(&mut self, mut record: LogRecord) {
        self.seq_counter += 1;
        record.seq = self.seq_counter;
        self.buffer[self.write_pos] = Some(record);
        self.write_pos = (self.write_pos + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
    }

    pub fn query(&self, params: &LogQuery) -> LogQueryResponse {
        let limit = params.limit.unwrap_or(100);
        let mut matches: Vec<&LogRecord> = Vec::new();

        let start = if self.count < self.capacity {
            0
        } else {
            self.write_pos
        };

        for i in 0..self.count {
            let idx = (start + i) % self.capacity;
            if let Some(ref record) = self.buffer[idx] {
                if self.matches_filter(record, params) {
                    matches.push(record);
                }
            }
        }

        let total_matches = matches.len();
        let records: Vec<LogRecord> = matches
            .into_iter()
            .rev()
            .take(limit)
            .rev()
            .cloned()
            .collect();

        LogQueryResponse {
            total_matches,
            records,
        }
    }

    fn matches_filter(&self, record: &LogRecord, params: &LogQuery) -> bool {
        if let Some(ref level) = params.level {
            if !self.level_matches(&record.level, level) {
                return false;
            }
        }
        if let Some(ref module) = params.module {
            if !record.target.starts_with(module) {
                return false;
            }
        }
        if let Some(since) = params.since {
            if record.timestamp < since {
                return false;
            }
        }
        if let Some(cursor) = params.cursor {
            if record.seq <= cursor {
                return false;
            }
        }
        if let Some(ref keyword) = params.keyword {
            if !record.message.contains(keyword) {
                return false;
            }
        }
        true
    }

    fn level_matches(&self, record_level: &str, min_level: &str) -> bool {
        let order = |l: &str| match l.to_uppercase().as_str() {
            "TRACE" => 0,
            "DEBUG" => 1,
            "INFO" => 2,
            "WARN" => 3,
            "ERROR" => 4,
            _ => 0,
        };
        order(record_level) >= order(min_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(level: &str, target: &str, message: &str, timestamp: i64) -> LogRecord {
        LogRecord {
            seq: 0,
            timestamp,
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
            fields: HashMap::new(),
            span_stack: Vec::new(),
        }
    }

    #[test]
    fn push_and_query_basic() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "mod_a", "hello", 1000));
        store.push(make_record("WARN", "mod_b", "world", 2000));

        let result = store.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.records[0].seq, 1);
        assert_eq!(result.records[1].seq, 2);
    }

    #[test]
    fn ring_buffer_overflow() {
        let mut store = LogStore::new(3);
        for i in 0..5 {
            store.push(make_record("INFO", "m", &format!("msg{}", i), i as i64));
        }
        let result = store.query(&LogQuery::default());
        assert_eq!(result.total_matches, 3);
        assert_eq!(result.records[0].message, "msg2");
        assert_eq!(result.records[2].message, "msg4");
    }

    #[test]
    fn filter_by_level() {
        let mut store = LogStore::new(100);
        store.push(make_record("DEBUG", "m", "dbg", 1000));
        store.push(make_record("WARN", "m", "wrn", 2000));
        store.push(make_record("ERROR", "m", "err", 3000));

        let result = store.query(&LogQuery {
            level: Some("WARN".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].level, "WARN");
        assert_eq!(result.records[1].level, "ERROR");
    }

    #[test]
    fn filter_by_module_prefix() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "alva_app_core::agent::engine", "a", 1000));
        store.push(make_record("INFO", "srow_ai::chat", "b", 2000));

        let result = store.query(&LogQuery {
            module: Some("alva_app_core".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].target, "alva_app_core::agent::engine");
    }

    #[test]
    fn filter_by_cursor() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "m", "first", 1000));
        store.push(make_record("INFO", "m", "second", 2000));
        store.push(make_record("INFO", "m", "third", 3000));

        let result = store.query(&LogQuery {
            cursor: Some(1),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].message, "second");
    }

    #[test]
    fn filter_by_keyword() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "m", "connection failed", 1000));
        store.push(make_record("INFO", "m", "all good", 2000));

        let result = store.query(&LogQuery {
            keyword: Some("failed".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].message, "connection failed");
    }

    #[test]
    fn limit_results() {
        let mut store = LogStore::new(100);
        for i in 0..10 {
            store.push(make_record("INFO", "m", &format!("msg{}", i), i as i64));
        }
        let result = store.query(&LogQuery {
            limit: Some(3),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 10);
        assert_eq!(result.records.len(), 3);
        assert_eq!(result.records[0].message, "msg7");
        assert_eq!(result.records[2].message, "msg9");
    }

    #[test]
    fn combined_filters() {
        let mut store = LogStore::new(100);
        store.push(make_record("DEBUG", "alva_app_core::agent", "step 1", 1000));
        store.push(make_record("WARN", "alva_app_core::agent", "step 2 failed", 2000));
        store.push(make_record("WARN", "srow_ai::chat", "chat failed", 3000));
        store.push(make_record("ERROR", "alva_app_core::mcp", "mcp error", 4000));

        let result = store.query(&LogQuery {
            level: Some("WARN".to_string()),
            module: Some("alva_app_core".to_string()),
            keyword: Some("failed".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].message, "step 2 failed");
    }
}
