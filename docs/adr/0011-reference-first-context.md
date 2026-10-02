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

Implementation is incremental. Attachments, Hub Desktop/server ALVA large-tool
projection, native-session history retrieval and managed single-entry MCP now
follow this rule. App-core recursive children now rebind this policy to their
own actual durable session, before communication assembly and provider calls;
they cannot regain removed root tools or read parent history. The optional
inheritance contract remains in the app layer, not kernel/agent-core. Other
engines' own tool loops, cross-engine product history and actual media-derived
operations remain outside this delivery. The tracked acceptance plan is
[ALVA-HUB-CONTEXT-RETRIEVAL.md](../ALVA-HUB-CONTEXT-RETRIEVAL.md).
