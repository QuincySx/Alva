// INPUT:  (none — string constants only)
// OUTPUT: CREATE_SESSIONS, CREATE_MESSAGES, CREATE_ACP_MESSAGES, CREATE_SCHEMA_VERSION, ALL_DDL
// POS:    DDL statements for the persistence layer's four tables.
//! DDL statements for persistence tables.

/// Sessions table — one row per agent session.
pub const CREATE_SESSIONS: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY NOT NULL,
    status          TEXT NOT NULL DEFAULT 'idle',
    workspace_path  TEXT NOT NULL,
    agent_type      TEXT NOT NULL DEFAULT '',
    config_snapshot TEXT NOT NULL DEFAULT '{}',
    total_tokens    INTEGER NOT NULL DEFAULT 0,
    iteration_count INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    last_active_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Messages table — ordered conversation history per session.
pub const CREATE_MESSAGES: &str = "
CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    msg_id       TEXT NOT NULL,
    role         TEXT NOT NULL,
    content_json TEXT NOT NULL,
    turn_index   INTEGER NOT NULL DEFAULT 0,
    token_count  INTEGER,
    tool_call_id TEXT,
    timestamp    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);
";

/// ACP messages table — inter-agent communication log.
pub const CREATE_ACP_MESSAGES: &str = "
CREATE TABLE IF NOT EXISTS acp_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    message_type    TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    timestamp       TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_acp_messages_conversation_ts ON acp_messages(conversation_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_acp_messages_type ON acp_messages(message_type);
";

/// Schema version tracking table.
pub const CREATE_SCHEMA_VERSION: &str = "
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
";

/// All DDL statements in order.
pub const ALL_DDL: &[&str] = &[
    CREATE_SESSIONS,
    CREATE_MESSAGES,
    CREATE_ACP_MESSAGES,
    CREATE_SCHEMA_VERSION,
];
