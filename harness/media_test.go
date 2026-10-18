// INPUT: Product request projection, real native session/artifacts and typed unselected tool media
// OUTPUT: All media reference-only, immutable original facts, bounded retrieval and fail-closed regressions
// POS: [L5 harness] Tool-returned images/audio/video never silently become model inputs or implicit remote fetches
package harness

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestEveryToolMediaUsesOwnedReferenceWithoutDelivery(t *testing.T) {
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(ctx, "owner", "actual-media", "")
	if err != nil {
		t.Fatal(err)
	}
	backend, err := repo.BoundReferences(ctx, "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	image := wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte("original small image byte fixture")}}
	content := []wire.ContentBlock{
		{Kind: wire.ContentText, Text: "small explicit tool status"},
		{Kind: wire.ContentImage, Image: &image},
		{Kind: wire.ContentAudio, Audio: &wire.Audio{Payload: wire.MediaPayload{MediaType: "audio/wav", URL: "https://never-fetched.example/audio.wav"}}},
		{Kind: wire.ContentVideo, Video: &wire.Video{Frames: []wire.VideoFrame{{TimestampMillis: 1234, Image: image}}}},
	}
	original := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "media-call", IsError: true, Content: content}}}}
	if _, err = log.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &original})}); err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(fixtureModelForArguments{}, log, nil)
	if err != nil {
		t.Fatal(err)
	}
	policy := referenceProjection{sessionID: log.ID(), backend: backend}
	var firstURIs []string
	for i := 0; i < 3; i++ {
		request := abi.ModelRequest{Messages: wire.CloneMessages([]wire.Message{original})}
		if err = policy.BeforeModel(ctx, abi.RuntimeContext{State: state}, &request); err != nil {
			t.Fatal(err)
		}
		result := request.Messages[0].Content[0].ToolResult
		if !result.IsError || result.CallID != "media-call" || result.Content[0].Text != "small explicit tool status" {
			t.Fatal("lost status/error identity")
		}
		for index, block := range result.Content[1:] {
			if block.Kind != wire.ContentReference || block.Reference == nil || !strings.Contains(block.Reference.Processing, "Not decoded") {
				t.Fatal("raw/remote tool media leaked into request", block.Kind)
			}
			if i == 0 {
				firstURIs = append(firstURIs, block.Reference.URI)
			} else if block.Reference.URI != firstURIs[index] {
				t.Fatal("repeated prepare stored another media artifact")
			}
			var restored strings.Builder
			for offset := int64(0); ; {
				page, e := backend.ReadText(ctx, block.Reference.URI, offset, 512)
				if e != nil {
					t.Fatal(e)
				}
				restored.WriteString(page.Text)
				if !page.Truncated {
					break
				}
				offset = page.NextOffset
			}
			encoded, _ := json.Marshal(content[index+1])
			if restored.String() != string(encoded) {
				t.Fatal("not full source DTO")
			}
		}
		// Reference projection itself is idempotent and never wraps references.
		before, _ := json.Marshal(request.Messages)
		if err = policy.BeforeModel(ctx, abi.RuntimeContext{State: state}, &request); err != nil {
			t.Fatal(err)
		}
		after, _ := json.Marshal(request.Messages)
		if !bytes.Equal(before, after) {
			t.Fatal("reference chain created")
		}
	}
	messages, err := log.Messages(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if messages[0].Content[0].ToolResult.Content[1].Kind != wire.ContentImage {
		t.Fatal("projection overwrote original fact")
	}
}

type mediaFailBackend struct {
	agentcontext.ReferenceBackend
	cause error
}

func (b mediaFailBackend) StoreText(context.Context, string) (wire.Reference, error) {
	return wire.Reference{}, b.cause
}

func TestMediaProjectionFailureCannotFallBackToOriginalPayload(t *testing.T) {
	cause := errors.New("real artifact write failed")
	image := wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte("fixture")}}
	request := abi.ModelRequest{Messages: []wire.Message{{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "call", Content: []wire.ContentBlock{{Kind: wire.ContentImage, Image: &image}}}}}}}}
	policy := referenceProjection{sessionID: "actual", backend: mediaFailBackend{cause: cause}}
	if err := policy.BeforeModel(context.Background(), testRuntime(t, "actual"), &request); !errors.Is(err, cause) {
		t.Fatal("lost actual storage failure", err)
	}
	image.Payload.Data = make([]byte, referenceStoreLimit*3/4+1)
	if err := policy.projectMedia(context.Background(), &wire.ContentBlock{Kind: wire.ContentImage, Image: &image}); err == nil || errors.Is(err, cause) {
		t.Fatal("oversized media not stopped before storage", err)
	}
}
