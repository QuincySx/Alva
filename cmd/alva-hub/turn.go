// INPUT: Explicit CLI prompt, actual committed branch/Turn ownership and scoped approval requests
// OUTPUT: Bounded terminal result only after successful owner completion, with cancellable HITL and exact native answer reads
// POS: [L7 CLI entry] Never treats deltas/early Completed as success or replays a failed task to recover display output.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const maxAnswer = 8 << 20

func (c *cli) turn(parent context.Context, prompt string) error {
	if strings.TrimSpace(prompt) == "" || len(prompt) > 32<<10 || !utf8.ValidString(prompt) {
		return errors.New("nonempty UTF-8 prompt must be<=32KiB")
	}
	detail, err := c.app.Store.Detail(parent, c.session)
	if err != nil {
		return err
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		return err
	}
	tail, err := c.app.Store.HistoryBefore(parent, c.session, 0, 1)
	if err != nil {
		return err
	}
	var after uint64
	if len(tail) > 0 {
		after = tail[0].Sequence
	}
	ctx, cancel := context.WithTimeout(parent, c.timeout)
	defer cancel()
	turn, done, err := c.app.Runtime.StartOnBranch(ctx, c.session, branch.ID, prompt)
	if err != nil {
		return err
	}
	settle := func(cause error) error {
		cancel()
		cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		cancelErr := c.app.Runtime.Cancel(cleanup, c.session)
		if errors.Is(cancelErr, hub.ErrSessionNotRunning) {
			cancelErr = nil
		}
		select {
		case err := <-done:
			return errors.Join(cause, cancelErr, err)
		case <-cleanup.Done():
			return errors.Join(cause, cancelErr, cleanup.Err())
		}
	}
	ticker := time.NewTicker(20 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case err := <-done:
			if err != nil {
				return err
			}
			if ctx.Err() != nil {
				return ctx.Err()
			}
			text, err := c.finalAnswer(ctx, turn, after)
			if err != nil {
				return err
			}
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return writeOutput(ctx, c.output, text+"\n")
		case <-ctx.Done():
			return settle(ctx.Err())
		case <-ticker.C:
			pending, err := c.app.Service.PendingApprovals(ctx, c.session)
			if err != nil {
				return settle(err)
			}
			for _, request := range pending {
				allow := false
				if c.ask {
					if _, err := fmt.Fprintf(c.diagnostic, "Permission %s · %s\n%s\nType allow %s to approve this call; anything else denies: ", request.ID, terminaltext.Escape(request.ToolName), terminaltext.Escape(request.InputSummary), request.ID); err != nil {
						return settle(err)
					}
					line, err := c.input.line(ctx)
					if err != nil {
						return settle(err)
					}
					allow = strings.TrimSpace(line) == "allow "+request.ID
				}
				if err := c.app.Service.RespondApproval(ctx, c.session, request.ID, allow); err != nil {
					return settle(err)
				}
			}
		}
	}
}

func (c *cli) finalAnswer(ctx context.Context, turn string, after uint64) (string, error) {
	var draft strings.Builder
	var attempt, id string
	var reference *engine.Reference
	succeeded := false
	for scanned := 0; scanned < 65536; {
		entries, err := c.app.Store.History(ctx, c.session, after, 128)
		if err != nil {
			return "", err
		}
		if len(entries) == 0 {
			break
		}
		for _, entry := range entries {
			after = entry.Sequence
			scanned++
			if entry.TurnID != turn || entry.AttemptID == "" {
				continue
			}
			if entry.AttemptID != attempt {
				attempt = entry.AttemptID
				id = ""
				draft.Reset()
				reference = nil
				succeeded = false
			}
			event := entry.Event
			switch event.Kind {
			case engine.Message, engine.MessageDelta:
				if event.ID != id || event.Kind == engine.Message {
					draft.Reset()
					reference = nil
					id = event.ID
				}
				if draft.Len()+len(event.Text) > maxAnswer {
					return "", errors.New("final output exceeds8MiB; inspect durable history, task was not replayed")
				}
				draft.WriteString(event.Text)
				if event.Kind == engine.Message {
					reference = event.Reference
				}
			case engine.ToolStart:
				draft.Reset()
				reference = nil
				id = ""
			case engine.Completed:
				succeeded = event.Outcome == engine.Succeeded
			}
		}
		if len(entries) < 128 {
			break
		}
		if scanned >= 65536 {
			return "", errors.New("final-answer history scan exceeded65536 entries; inspect durable history")
		}
	}
	if !succeeded {
		return "", errors.New("successful final attempt was not present in durable history")
	}
	if reference != nil && reference.Sequence > 0 {
		// The product Message is a small display preview. Read only its exact
		// qualified committed native record, not all session messages/history.
		detail, err := c.app.Store.Detail(ctx, c.session)
		if err != nil {
			return "", err
		}
		owned := false
		for _, branch := range detail.Branches {
			if branch.Target.EngineID == "alva" && branch.NativeSessionID == reference.SessionID {
				owned = true
			}
		}
		if !owned {
			return "", errors.New("final native answer is not owned by the saved product branch")
		}
		page, err := c.app.Repository.Page(ctx, c.session, reference.SessionID, reference.Sequence-1, 1, reference.Sequence)
		if err != nil {
			return "", err
		}
		if len(page.Entries) != 1 || page.Entries[0].ID != reference.RecordID || page.Entries[0].Value.Bytes > maxAnswer {
			return "", errors.New("exact final native answer metadata missing or oversized")
		}
		_, data, err := c.app.Repository.ReadArtifactFull(ctx, c.session, reference.SessionID, page.Entries[0].Value.URI, maxAnswer)
		if err != nil {
			return "", err
		}
		var record abi.SessionRecord
		if json.Unmarshal(data, &record) != nil || record.ID != reference.RecordID || record.Kind != abi.RecordConversation || record.Message == nil || record.Message.Role != wire.RoleAssistant {
			return "", errors.New("qualified final native record is not the committed assistant answer")
		}
		for _, block := range record.Message.Content {
			if block.Kind == wire.ContentToolCall {
				return "", errors.New("native display reference points to a tool-call preface, not a final answer")
			}
		}
		text := record.Message.Text()
		if len(text) > maxAnswer {
			return "", errors.New("final native answer exceeds8MiB")
		}
		return text, nil
	}
	return draft.String(), nil
}
