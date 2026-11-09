// INPUT: The Hub session list, pending approvals/questions of running sessions and MyGo notification and window-focus facts
// OUTPUT: attentionTracker (which transitions deserve a notification), the app-wide notifier and the click-to-session mapping
// POS: [L7 Hub UI] One notifier per application, not per window; it only reads product state and never answers or controls a session
//go:build desktop

package hubdesktop

import (
	"context"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo"
)

const attentionIDPrefix = "session:"

// attentionID is the notification identity for a session. A newer
// notification of the same session replaces the one still shown.
func attentionID(session string) string { return attentionIDPrefix + session }

// attentionSession maps a clicked notification ID back to its session.
func attentionSession(id string) (string, bool) {
	session, ok := strings.CutPrefix(id, attentionIDPrefix)
	return session, ok && session != ""
}

// attentionTitle is every notification's title: a session title can be private,
// and notifications reach lock screens and other devices.
const attentionTitle = "Alva"

// attention is one thing the user is being told about. Bodies are fixed
// phrases: approval and question text may carry sensitive content, so it
// never reaches the notification centre.
type attention struct {
	Session, Title, Body string
}

// pendingRequests are the approval and question IDs one running session waits on.
type pendingRequests struct{ Approvals, Questions []string }

// attentionTracker diffs consecutive observations. The first observation only
// primes it, so opening the app never replays history. It is confined to the
// observer goroutine.
type attentionTracker struct {
	primed  bool
	status  map[string]engine.Outcome
	pending map[string]bool // session+"/"+request ID, pruned to what is still pending
}

func newAttentionTracker() *attentionTracker {
	return &attentionTracker{status: map[string]engine.Outcome{}, pending: map[string]bool{}}
}

func (t *attentionTracker) observe(sessions []hub.Session, requests map[string]pendingRequests) []attention {
	var out []attention
	nextPending := map[string]bool{}
	for _, session := range sessions {
		previous, known := t.status[session.ID]
		active := hub.BlocksInput(previous)
		if t.primed && known && active {
			switch session.Status {
			case engine.Succeeded:
				out = append(out, attention{session.ID, attentionTitle, "回复已完成"})
			case engine.Failed:
				out = append(out, attention{session.ID, attentionTitle, "任务失败，需要查看"})
			case engine.Waiting:
				if previous != engine.Waiting {
					out = append(out, attention{session.ID, attentionTitle, "任务在等待你的处理"})
				}
			}
		}
		pending := requests[session.ID]
		for _, id := range pending.Approvals {
			key := session.ID + "/approval/" + id
			nextPending[key] = true
			if t.primed && !t.pending[key] {
				out = append(out, attention{session.ID, attentionTitle, "需要你的审批"})
			}
		}
		for _, id := range pending.Questions {
			key := session.ID + "/question/" + id
			nextPending[key] = true
			if t.primed && !t.pending[key] {
				out = append(out, attention{session.ID, attentionTitle, "需要你的回答"})
			}
		}
		t.status[session.ID] = session.Status
	}
	// Forget sessions that left the observed page (deleted, or older than it).
	seen := make(map[string]bool, len(sessions))
	for _, session := range sessions {
		seen[session.ID] = true
	}
	for id := range t.status {
		if !seen[id] {
			delete(t.status, id)
		}
	}
	t.pending = nextPending
	t.primed = true
	return out
}

// attentionObserver reads the newest sessions through the product's narrow
// services and shows system notifications while no window of the application
// is in the foreground. show and foreground are injectable for tests.
type attentionObserver struct {
	service    *product.HubService
	tracker    *attentionTracker
	foreground func() bool
	show       func(attention)
}

func newAttentionObserver(service *product.HubService) *attentionObserver {
	return &attentionObserver{
		service:    service,
		tracker:    newAttentionTracker(),
		foreground: func() bool { return mygo.FocusedWindow() != nil },
		show:       showSystemNotification,
	}
}

// poll performs one observation. Running sessions are the only ones that can
// hold a live approval or question, so only they are asked.
func (o *attentionObserver) poll(ctx context.Context) {
	page, err := o.service.Conversations.Page(ctx, "", hub.SessionCursor{}, 32)
	if err != nil {
		return
	}
	requests := map[string]pendingRequests{}
	for _, session := range page.Sessions {
		if session.Status != hub.Running {
			continue
		}
		var pending pendingRequests
		if approvals, err := o.service.PendingApprovals(ctx, session.ID); err == nil {
			for _, request := range approvals {
				pending.Approvals = append(pending.Approvals, request.ID)
			}
		}
		if questions, err := o.service.PendingQuestions(ctx, session.ID); err == nil {
			for _, question := range questions {
				pending.Questions = append(pending.Questions, question.ID)
			}
		}
		requests[session.ID] = pending
	}
	events := o.tracker.observe(page.Sessions, requests)
	if o.foreground() {
		return // the user is looking at the app: nothing to announce
	}
	for _, event := range events {
		o.show(event)
	}
}

func (o *attentionObserver) run(ctx context.Context, every time.Duration) {
	ticker := time.NewTicker(every)
	defer ticker.Stop()
	for {
		o.poll(ctx)
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

func showSystemNotification(a attention) {
	if !mygo.NotificationsSupported() {
		return
	}
	// A denied permission or a delivery failure is not an application error.
	_ = mygo.NewNotification(mygo.NotificationOptions{ID: attentionID(a.Session), Title: a.Title, Body: a.Body, Group: attentionID(a.Session)}).Show()
}
