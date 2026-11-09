// INPUT: Explicit CLI prompt, actual committed branch/Turn ownership and scoped approval requests
// OUTPUT: Bounded terminal result only after successful owner completion, with cancellable HITL and exact native answer reads
// POS: [L7 CLI entry] Never treats deltas/early Completed as success or replays a failed task to recover display output.
package main

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/sqlite"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const maxAnswer = 8 << 20

func (c *cli) turn(parent context.Context, prompt string) (result error) {
	if strings.TrimSpace(prompt) == "" || len(prompt) > 32<<10 || !utf8.ValidString(prompt) {
		return errors.New("nonempty UTF-8 prompt must be<=32KiB")
	}
	conversations, turns := c.app.Service.Conversations, c.app.Service.Turns
	tail, err := conversations.HistoryBefore(parent, c.session, 0, 1)
	if err != nil {
		return err
	}
	var after uint64
	if len(tail) > 0 {
		after = tail[0].Sequence
	}
	ctx, cancel := context.WithTimeout(parent, c.timeout)
	defer cancel()
	// SQLite reports an interrupt caused by this Turn's own deadline or Stop as a
	// bare driver error; keep the owner's cancellation identity so a timeout is
	// never mistaken for (or printed as) another failure.
	defer func() { result = ownTurnCancellation(ctx, result) }()
	// The CLI process owns this execution: its timeout cancels the run, and
	// Stop names the identity this admission returned.
	start, err := turns.Start(ctx, c.session, prompt)
	if err != nil {
		return err
	}
	turn, done := start.TurnID, start.Done
	settle := func(cause error) error {
		cancel()
		cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		cancelErr := turns.Stop(cleanup, c.session, start.ExecutionID)
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
	handledNative := make(map[string]bool)
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
			// Native engine interactions come only from a ready external owner;
			// the product returns none for an Alva branch or an idle session.
			interactions, err := c.app.Service.PendingEngineInteractions(ctx, c.session)
			if err != nil {
				return settle(err)
			}
			for _, request := range interactions {
				key := request.Handle.ID()
				if handledNative[key] {
					continue
				}
				switch request.Event.Kind {
				case engine.PermissionRequest:
					allow := false
					if c.ask {
						if _, err := fmt.Fprintf(c.diagnostic, "Native permission %s · %s\n%s\nType allow %s to approve this call; anything else denies: ", key, terminaltext.Escape(request.Event.Name), terminaltext.Escape(string(request.Event.Input)), key); err != nil {
							return settle(err)
						}
						line, err := c.input.line(ctx)
						if err != nil {
							return settle(err)
						}
						allow = strings.TrimSpace(line) == "allow "+key
					}
					if err := c.app.Service.RespondEnginePermission(ctx, c.session, request.Handle, allow); err != nil {
						return settle(err)
					}
					handledNative[key] = true
				case engine.UserInputRequested:
					return settle(fmt.Errorf("%w: native human questions require Hub desktop", engine.ErrUnsupported))
				}
			}
		}
	}
}

// ownTurnCancellation joins the Turn context's error onto a bare SQLite
// cancellation side effect so exit handling sees DeadlineExceeded/Canceled.
func ownTurnCancellation(ctx context.Context, err error) error {
	if err != nil && ctx.Err() != nil && sqlite.CancelEffect(err) && !errors.Is(err, ctx.Err()) {
		return errors.Join(err, ctx.Err())
	}
	return err
}

func (c *cli) finalAnswer(ctx context.Context, turn string, after uint64) (string, error) {
	var draft strings.Builder
	var attempt, id string
	var reference *engine.Reference
	succeeded := false
	for scanned := 0; scanned < 65536; {
		entries, err := c.app.Service.Conversations.History(ctx, c.session, after, 128)
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
		record, err := c.app.Service.Conversations.ReadNativeRecord(ctx, c.session, *reference, maxAnswer)
		if err != nil {
			return "", err
		}
		if record.Kind != abi.RecordConversation || record.Message == nil || record.Message.Role != wire.RoleAssistant {
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
