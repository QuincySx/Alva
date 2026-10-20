// INPUT: context, fmt, errors, testing, kernel/abi and llm/wire.
// OUTPUT: Fixed-water, owned metadata, hash/protocol admission and cold omitted-body regressions.
// POS: [Native storage testing] Verifies bounded storage primitives without model-context policy or prompt wording.
package session

import (
	"errors"
	"fmt"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestConversationTailFixedWaterAndDetachedMetadata(t *testing.T) {
	repo, log := repoSession(t)
	for i := 0; i < 10; i++ {
		if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(conversation(fmt.Sprint(i)))}); err != nil {
			t.Fatal(err)
		}
	}
	tail, err := repo.BoundConversationTail(t.Context(), "owner", log.ID(), 4)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := tail.Metadata()
	if err != nil || len(metadata) != 5 || tail.HighWater() != 10 {
		t.Fatal(metadata, err)
	}
	metadata[0].Reference.URI = "foreign"
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(conversation("later"))}); err != nil {
		t.Fatal(err)
	}
	view, err := tail.Read(t.Context(), 4, tailMaxBytes)
	if err != nil || len(view) != 4 || view[3].Text() != "9" {
		t.Fatal("snapshot changed", view, err)
	}
	if _, err := repo.BoundConversationTail(t.Context(), "foreign", log.ID(), 4); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("foreign authority", err)
	}
	if _, err := tail.Read(t.Context(), 6, tailMaxBytes); err == nil {
		t.Fatal("read outside snapshot")
	}
}

func TestExistingRepositoryGainsConversationIndexWithoutChangingFacts(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "facts")
	repo, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(t.Context(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(conversation("retained"))}); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.db.ExecContext(t.Context(), "DROP INDEX conversation_tail"); err != nil {
		t.Fatal(err)
	}
	if err := repo.Close(); err != nil {
		t.Fatal(err)
	}
	repo, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	var indexed int
	if err := repo.db.QueryRowContext(t.Context(), "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='conversation_tail'").Scan(&indexed); err != nil || indexed != 1 {
		t.Fatal("index not restored", indexed, err)
	}
	tail, err := repo.BoundConversationTail(t.Context(), "owner", "native", 1)
	if err != nil {
		t.Fatal(err)
	}
	if messages, err := tail.Read(t.Context(), 1, tailMaxBytes); err != nil || len(messages) != 1 || messages[0].Text() != "retained" {
		t.Fatal("facts changed on index upgrade", messages, err)
	}
}

func TestConversationTailRejectsInvalidSelectedEvidence(t *testing.T) {
	for _, failure := range []string{"bytes", "missing", "body", "orphan"} {
		t.Run(failure, func(t *testing.T) {
			repo, log := repoSession(t)
			if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(conversation("latest"))}); err != nil {
				t.Fatal(err)
			}
			switch failure {
			case "bytes":
				_, _ = repo.db.ExecContext(t.Context(), "UPDATE artifacts SET bytes=? WHERE session_id=?", tailMaxBytes+1, log.ID())
			case "missing":
				_, _ = repo.db.ExecContext(t.Context(), "PRAGMA foreign_keys=OFF")
				_, _ = repo.db.ExecContext(t.Context(), "DELETE FROM artifacts WHERE session_id=?", log.ID())
			case "body":
				_, _ = repo.db.ExecContext(t.Context(), "UPDATE artifact_chunks SET data=?", []byte("corrupt"))
			case "orphan":
				message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "missing"}}}}
				if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})}); err != nil {
					t.Fatal(err)
				}
			}
			tail, err := repo.BoundConversationTail(t.Context(), "owner", log.ID(), tailMaxMessages)
			if err != nil {
				t.Fatal(err)
			}
			messages, err := tail.Read(t.Context(), len(tail.(*conversationTail).metadata), tailMaxBytes)
			if err == nil || messages != nil {
				t.Fatal("invalid partial read accepted", messages, err)
			}
		})
	}
}
func TestConversationTailLeavesOmittedBodiesCold(t *testing.T) {
	repo, log := repoSession(t)
	for i := 0; i < 128; i++ {
		if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(conversation(fmt.Sprint(i)))}); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := repo.db.ExecContext(t.Context(), "UPDATE artifact_chunks SET data=? WHERE artifact_id=(SELECT artifact_id FROM records WHERE session_id=? AND seq=1)", []byte("corrupt omitted"), log.ID()); err != nil {
		t.Fatal(err)
	}
	if _, err := log.Messages(t.Context()); err == nil {
		t.Fatal("fixture not corrupt")
	}
	tail, err := repo.BoundConversationTail(t.Context(), "owner", log.ID(), 64)
	if err != nil {
		t.Fatal(err)
	}
	if view, err := tail.Read(t.Context(), 64, tailMaxBytes); err != nil || len(view) != 64 {
		t.Fatal("omitted body restored", len(view), err)
	}
}
