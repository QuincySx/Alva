// INPUT: Explicit saved-workspace paths, bounded metadata queries and existing ordinary-session facts.
// OUTPUT: Optional read-only workspace navigation and global automation browsing contracts.
// POS: [L7 Hub product] Presentation indexes do not expand ExecutionStore or create projects, sessions or tasks.
package hub

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type WorkspaceCursor struct {
	Path string `json:"path"`
}

type WorkspaceEntry struct {
	Path         string `json:"path"`
	Name         string `json:"name"`
	SessionCount int    `json:"session_count"`
	LastActiveAt int64  `json:"last_active_at"`
}

type WorkspacePage struct {
	Items   []WorkspaceEntry `json:"items"`
	Next    WorkspaceCursor  `json:"next"`
	HasMore bool             `json:"has_more"`
}

// ExcludeRoot is a location filter, not evidence of how a workspace was
// originally chosen. The product uses it to omit its managed private tree.
type WorkspaceNavigationQuery struct {
	Query       string
	ExcludeRoot string
}

func ValidateWorkspaceNavigation(query WorkspaceNavigationQuery, cursor WorkspaceCursor, limit int) error {
	if limit < 1 || limit > 128 || len(query.Query) > 256 || !utf8.ValidString(query.Query) || strings.ContainsRune(query.Query, 0) {
		return errors.New("bounded workspace navigation query and limit required")
	}
	for _, path := range []string{query.ExcludeRoot, cursor.Path} {
		if path != "" && (len(path) > 4096 || !utf8.ValidString(path) || strings.ContainsRune(path, 0) || !filepath.IsAbs(path) || filepath.Clean(path) != path) {
			return errors.New("workspace navigation requires clean absolute paths")
		}
	}
	return nil
}

// Only saved ordinary-session workspace metadata is indexed. Empty projects,
// filesystem discovery and identity/credential discovery are not implied.
type WorkspaceNavigationStore interface {
	PageWorkspaces(context.Context, WorkspaceNavigationQuery, WorkspaceCursor, int) (WorkspacePage, error)
	PageWorkspaceSessions(context.Context, string, string, SessionCursor, int) (SessionPage, error)
}

// The existing global 100-configuration budget also bounds this cold index.
// Listing never ticks the scheduler or changes a pinned execution route.
type AutomationBrowseStore interface {
	ListAutomationIndex(context.Context) ([]Automation, error)
}
