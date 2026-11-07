// INPUT: MyGo's opt-in MYGO_AUTOMATION automation variable plus the page's existing optional logger
// OUTPUT: One shared stderr numeric trace logger for signed e2e builds that opted into automation
// POS: [L7 Hub UI] Reuses the existing automation opt-in; adds no new flag, protocol, timer, polling or behavior.
//go:build desktop && e2e

package hubdesktop

import (
	"log/slog"
	"os"
	"sync"
)

// A signed e2e build can run without the debug server, which is exactly when the
// automation harness needs the search phases. Fall back to stderr only under the
// same MYGO_AUTOMATION opt-in MyGo already requires, shared once so concurrent
// windows still write whole lines rather than interleaved fragments.
var workspaceSearchFallbackLogger = sync.OnceValue(func() *slog.Logger {
	if os.Getenv("MYGO_AUTOMATION") == "" {
		return nil
	}
	return slog.New(slog.NewJSONHandler(os.Stderr, nil))
})

func workspaceSearchTraceLogger(existing *slog.Logger) *slog.Logger {
	if existing != nil {
		return existing
	}
	return workspaceSearchFallbackLogger()
}
