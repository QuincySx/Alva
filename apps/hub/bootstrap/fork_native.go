// INPUT: context, strings, storage.Repository tail reads, kernel/abi session records, llm/wire and hub.NativeOwnerResolver
// OUTPUT: forkWindow/cutForkWindow, copyForkedConversation and checkForkedConversation: the absolute-anchored cut copy of a source native conversation that starts a forked Alva session
// POS: [L7 Hub bootstrap] Alva's native fork: the cut is an absolute native record sequence fixed when the intent was recorded, so a source that grew meanwhile or holds fewer prompts than the product has Turns cannot move it; nothing is copied from the product side
package bootstrap

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const (
	forkTailMessages = 512
	forkTailBytes    = 8 << 20
	forkAppendBatch  = 32
)

// errForkUnreachable says the cut lies before the part of the source Alva can
// read (its bounded tail), so no faithful copy exists.
var errForkUnreachable = fmt.Errorf("%w: the cut lies before the readable conversation tail or does not match the source", engine.ErrForkUnreachable)

// forkOwners resolves which product session owns a native conversation.
type forkOwners interface {
	hub.NativeOwnerResolver
}

// forkWindow is the source's newest conversation messages with the native
// record sequence of each, oldest first.
type forkWindow struct {
	messages  []wire.Message
	sequences []uint64
	// truncated: older messages exist that the window does not hold.
	truncated bool
}

func loadForkWindow(ctx context.Context, repository storage.Repository, owners forkOwners, fork engine.ForkPoint) (forkWindow, error) {
	var window forkWindow
	if err := fork.Validate(); err != nil {
		return window, err
	}
	owner := fork.OwnerID
	if owner == "" {
		resolved, err := owners.NativeOwner(ctx, "alva", fork.Session)
		if err != nil {
			return window, errors.New("fork source conversation is not owned by a saved product branch")
		}
		owner = resolved
	}
	tail, err := repository.BoundConversationTail(ctx, owner, fork.Session, forkTailMessages)
	if err != nil {
		return window, err
	}
	metadata, err := tail.Metadata()
	if err != nil {
		return window, err
	}
	count := min(len(metadata), forkTailMessages)
	window.truncated = len(metadata) > forkTailMessages
	if count == 0 {
		return window, nil
	}
	if window.messages, err = tail.Read(ctx, count, forkTailBytes); err != nil {
		return window, err
	}
	if len(window.messages) != count {
		return window, errors.New("fork source tail is inconsistent")
	}
	// Metadata is newest first; messages are oldest first.
	for i := count - 1; i >= 0; i-- {
		window.sequences = append(window.sequences, metadata[i].Record.Sequence)
	}
	return window, nil
}

// cutForkWindow returns the messages a fork keeps. A cut with ThroughSequence
// keeps every message up to that native record. A cut with BeforeSequence
// drops the last prompt (a user message that is not an interjection)
// committed below it and everything after: BeforeSequence is the first record
// the dropped Turn produced, so the prompt sits just under it. A cut the
// readable tail does not reach is an error, never an empty context.
func cutForkWindow(window forkWindow, fork engine.ForkPoint) ([]wire.Message, error) {
	if fork.ThroughSequence != 0 {
		if window.truncated && window.sequences[0] > fork.ThroughSequence {
			return nil, errForkUnreachable
		}
		end := 0
		for end < len(window.messages) && window.sequences[end] <= fork.ThroughSequence {
			end++
		}
		return window.messages[:end], nil
	}
	if fork.BeforeSequence == 0 {
		return nil, errors.New("fork has no native anchor")
	}
	for i := len(window.messages) - 1; i >= 0; i-- {
		message := window.messages[i]
		if window.sequences[i] >= fork.BeforeSequence || message.Role != wire.RoleUser || strings.HasPrefix(message.ID, alva.InterjectionMessagePrefix) {
			continue
		}
		return window.messages[:i], nil
	}
	// No prompt below the anchor: the source does not match what the product
	// recorded (or the prompt is older than the readable tail). Never an
	// empty context.
	return nil, errForkUnreachable
}

// checkForkedConversation answers engine.ForkChecker: reachable or not,
// creating nothing.
func checkForkedConversation(ctx context.Context, repository storage.Repository, owners forkOwners, fork engine.ForkPoint) error {
	window, err := loadForkWindow(ctx, repository, owners, fork)
	if err != nil {
		return err
	}
	_, err = cutForkWindow(window, fork)
	return err
}

// prepareForkedConversation reads the source and decides the cut without
// creating anything: the messages the new native session will start with.
func prepareForkedConversation(ctx context.Context, repository storage.Repository, owners forkOwners, request engine.Request) ([]wire.Message, error) {
	fork := request.Fork
	if fork == nil {
		return nil, nil
	}
	window, err := loadForkWindow(ctx, repository, owners, *fork)
	if err != nil {
		return nil, err
	}
	return cutForkWindow(window, *fork)
}

// writeForkedConversation writes the kept messages (at most 512 and 8 MiB, all
// the context Alva ever reads back) into the new native session. The source is
// never changed.
func writeForkedConversation(ctx context.Context, target storage.Session, kept []wire.Message) error {
	for start := 0; start < len(kept); start += forkAppendBatch {
		end := min(start+forkAppendBatch, len(kept))
		batch := make([]abi.SessionRecord, 0, end-start)
		for i := start; i < end; i++ {
			message := kept[i]
			batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
		}
		if _, err := target.AppendRecords(ctx, batch); err != nil {
			return err
		}
	}
	return nil
}
