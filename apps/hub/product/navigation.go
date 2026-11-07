// INPUT: Optional shared navigation ports and the product's explicit managed workspace location.
// OUTPUT: Validated cold project/session pages and bounded global automation snapshots.
// POS: [L7 product facade] Reuses stored facts without filesystem scans, new execution or credential discovery.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *HubService) BrowseWorkspaces(ctx context.Context, query string, cursor hub.WorkspaceCursor, limit int) (hub.WorkspacePage, error) {
	filter := hub.WorkspaceNavigationQuery{Query: query, ExcludeRoot: filepath.Join(s.DataDirectory, "workspaces")}
	if err := hub.ValidateWorkspaceNavigation(filter, cursor, limit); err != nil {
		return hub.WorkspacePage{}, err
	}
	port, ok := s.Store.(hub.WorkspaceNavigationStore)
	if !ok {
		return hub.WorkspacePage{}, engine.ErrUnsupported
	}
	page, err := port.PageWorkspaces(ctx, filter, cursor, limit)
	if err != nil {
		return hub.WorkspacePage{}, err
	}
	if len(page.Items) > limit || len(page.Items) == 0 && page.HasMore {
		return hub.WorkspacePage{}, errors.New("workspace navigation returned an invalid page")
	}
	page.Items = append([]hub.WorkspaceEntry(nil), page.Items...)
	previous := cursor.Path
	for i := range page.Items {
		entry := &page.Items[i]
		if hub.ValidateWorkspaceNavigation(hub.WorkspaceNavigationQuery{}, hub.WorkspaceCursor{Path: entry.Path}, 1) != nil || entry.Path == "" || entry.Path <= previous || entry.SessionCount <= 0 || entry.LastActiveAt < 0 {
			return hub.WorkspacePage{}, errors.New("workspace navigation returned invalid stored metadata")
		}
		relative, relErr := filepath.Rel(filter.ExcludeRoot, entry.Path)
		if relErr == nil && (relative == "." || relative != ".." && !filepath.IsAbs(relative) && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
			return hub.WorkspacePage{}, errors.New("workspace navigation included the excluded managed location")
		}
		entry.Name = filepath.Base(entry.Path)
		previous = entry.Path
	}
	if len(page.Items) != 0 && page.Next.Path != previous {
		return hub.WorkspacePage{}, errors.New("workspace navigation changed its cursor")
	}
	return page, nil
}

func (s *HubService) BrowseWorkspaceSessions(ctx context.Context, workspace, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	if workspace == "" || hub.ValidateWorkspaceNavigation(hub.WorkspaceNavigationQuery{Query: query}, hub.WorkspaceCursor{Path: workspace}, limit) != nil {
		return hub.SessionPage{}, errors.New("workspace session navigation requires an explicit saved workspace")
	}
	port, ok := s.Store.(hub.WorkspaceNavigationStore)
	if !ok {
		return hub.SessionPage{}, engine.ErrUnsupported
	}
	page, err := port.PageWorkspaceSessions(ctx, workspace, query, cursor, limit)
	if err != nil {
		return hub.SessionPage{}, err
	}
	if len(page.Sessions) > limit || len(page.Sessions) == 0 && page.HasMore {
		return hub.SessionPage{}, errors.New("workspace session navigation returned an invalid page")
	}
	previous := cursor
	seen := make(map[string]bool, len(page.Sessions))
	for _, session := range page.Sessions {
		if session.Workspace != workspace || session.ID == "" || seen[session.ID] || session.UpdatedAt <= 0 {
			return hub.SessionPage{}, errors.New("workspace session navigation changed its scope")
		}
		if previous.Updated != 0 && (session.UpdatedAt > previous.Updated || session.UpdatedAt == previous.Updated && session.ID >= previous.ID) {
			return hub.SessionPage{}, errors.New("workspace session navigation changed its keyset order")
		}
		seen[session.ID] = true
		previous = hub.SessionCursor{Updated: session.UpdatedAt, ID: session.ID}
	}
	if len(page.Sessions) != 0 && page.Next != previous {
		return hub.SessionPage{}, errors.New("workspace session navigation changed its cursor")
	}
	page.Sessions = append([]hub.Session(nil), page.Sessions...)
	return page, nil
}

func (s *HubService) BrowseAutomations(ctx context.Context) ([]hub.Automation, error) {
	port, ok := s.Store.(hub.AutomationBrowseStore)
	if !ok {
		return nil, engine.ErrUnsupported
	}
	items, err := port.ListAutomationIndex(ctx)
	if err != nil {
		return nil, err
	}
	if len(items) > 100 {
		return nil, errors.New("automation index exceeds its 100-config budget")
	}
	items = append([]hub.Automation(nil), items...)
	seen := make(map[string]bool, len(items))
	for i := range items {
		item := &items[i]
		if item.ID == "" || item.SessionID == "" || item.BranchID == "" || item.Revision == 0 || seen[item.ID] || item.AutomationDraft.Validate() != nil || item.Target.Validate() != nil {
			return nil, errors.New("automation index returned invalid stored metadata")
		}
		seen[item.ID] = true
		item.Target.RequiredCapabilities = append([]string(nil), item.Target.RequiredCapabilities...)
	}
	return items, nil
}
