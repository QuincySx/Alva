// INPUT: The page's existing optional diagnostic logger, ordinary desktop builds only
// OUTPUT: Trace logger resolution that stays inert when no diagnostic owner exists
// POS: [L7 Hub UI] Reuses the app-owned logger only; adds no setting, env flag, output channel or behavior.
//go:build desktop && !e2e

package hubdesktop

import "log/slog"

// An ordinary desktop build emits search diagnostics solely into the logger the
// debug owner already handed the page. Without that owner every phase call is
// inert: no output, no extra state, no repaint and no timing change.
func workspaceSearchTraceLogger(existing *slog.Logger) *slog.Logger { return existing }
