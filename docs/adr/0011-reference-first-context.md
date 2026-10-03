# Reference-first context for files, MCP and history

Status: accepted, 2026-10-02 (explicit user direction).

Uploaded logs/media, every tool's large results, MCP catalogs and past conversation records must not be
automatically copied wholesale into model context. The Hub/Plugin layer stores
the authoritative content, provides a short reference/index, and lets the agent
search and retrieve bounded relevant excerpts according to the user's task.
This makes context size a retrieval budget rather than a side effect of how
large an upload, server catalog or conversation happens to be.

The stable kernel and agent-core do not gain product retrieval policy. Existing
permissions, ownership checks and truthful error/result status remain mandatory;
referenced files and external descriptions are data, not higher-priority
instructions. Explicit user authorization still gates cross-session scope.

Clarified 2026-10-03: managed MCP interactions run in an owned background I/O
worker, not another model or Hub WorkerSession. Complete accepted, redacted
protocol/gateway inputs, outputs, notifications and stderr are retained in memory
and the same native session's durable files; the calling agent receives references
and searches/reads relevant text itself. JSON-path/structured-field extraction is
deliberately not planned. Protocol/storage budgets stop the connection explicitly
instead of silently discarding accepted output; outgoing attempts and interrupted
transcripts do not prove remote acceptance. Generic callers keep their opt-in
compatibility path. Parent-bound recorded gateways cannot be copied into children
as a way to write/read the parent's journal. An optional app-level MCP spawn
policy now creates an independent recorded gateway only after the direct parent
actually grants it, using the approved server/cwd snapshot and actual durable
child authority. Children and grandchildren own their workers and journals;
startup observes parent cancellation and consumes the child's existing budget.
An absent grant does not start host I/O or restore root tools; failed binding
does not fall back to the parent's gateway. Shared security/HITL remains intact.

Implementation is incremental. Attachments, Hub Desktop/server ALVA large-tool
projection, native-session history retrieval and managed single-entry MCP now
follow this rule. App-core recursive children now rebind this policy to their
own actual durable session, before communication assembly and provider calls;
they cannot regain removed root tools or read parent history. The optional
inheritance contract remains in the app layer, not kernel/agent-core. Other
engines' own tool loops remain outside this delivery. The claimed HubSession now
adds a root-bound product Plugin for cross-branch visible history and uploaded
text retrieval, without global/Worker/parent access. Explicit native media tools
create separately hashed derivatives; only selected bounded JPEGs enter one
declared image-capable model request, with durable receipts and no auto-replay.
Processing, attempted submission, provider response and understanding are not
interchangeable evidence. Video/audio originals are not attached automatically.
The tracked acceptance plan and remaining scope are in
[ALVA-HUB-CONTEXT-RETRIEVAL.md](../ALVA-HUB-CONTEXT-RETRIEVAL.md).
