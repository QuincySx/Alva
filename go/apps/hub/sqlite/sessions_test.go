// INPUT: Real private session metadata and keyset cursors
// OUTPUT: Complete bounded home pagination and literal title/workspace search regressions
// POS: [L7 Hub product] Does not load native facts or execute engines
package sqlite

import (
	"context"
	"fmt"
	"github.com/QuincySx/Alva/go/apps/hub"
	"testing"
)

func TestSessionHomeKeysetPagesAndLiteralSearch(t *testing.T) {
	store := newStore(t)
	ctx := context.Background()
	workspace := t.TempDir()
	for i := 0; i < 21; i++ {
		if _, err := store.CreateSession(ctx, fmt.Sprintf("session-%d", i), workspace, target("a")); err != nil {
			t.Fatal(err)
		}
	}
	seen := map[string]bool{}
	cursor := hub.SessionCursor{}
	for pageNo := 0; pageNo < 10; pageNo++ {
		page, err := store.PageSessions(ctx, "", cursor, 4)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Sessions) > 4 {
			t.Fatal("page over budget")
		}
		for _, session := range page.Sessions {
			if seen[session.ID] {
				t.Fatal("keyset page duplicated a session")
			}
			seen[session.ID] = true
		}
		if !page.HasMore {
			break
		}
		cursor = page.Next
	}
	if len(seen) != 21 {
		t.Fatal("older sessions inaccessible", len(seen))
	}
	page, err := store.PageSessions(ctx, "session-20", hub.SessionCursor{}, 4)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].Title != "session-20" {
		t.Fatal("literal title search failed", err)
	}
	page, err = store.PageSessions(ctx, workspace, hub.SessionCursor{}, 4)
	if err != nil || len(page.Sessions) != 4 || !page.HasMore {
		t.Fatal("workspace search did not page", err)
	}
}
