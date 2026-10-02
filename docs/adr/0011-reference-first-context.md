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

Implementation is incremental. Attachments now follow this rule; MCP still
registers every discovered tool and returns full results, and model-facing
history retrieval and the generic large-tool-output policy are not yet implemented. The tracked acceptance plan is
[ALVA-HUB-CONTEXT-RETRIEVAL.md](../ALVA-HUB-CONTEXT-RETRIEVAL.md).
