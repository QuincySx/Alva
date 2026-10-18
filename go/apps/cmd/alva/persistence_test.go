// INPUT: bytes, context, encoding/json, net/http/httptest, os, filepath, testing, storage/session, llm/wire.
// OUTPUT: Explicit durable CLI resume/history and reference-only attachment regressions.
// POS: [L6 application] Local session tests without legacy data, remote providers or media processing.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"

	memorysqlite "github.com/QuincySx/Alva/go/platform/storage/memory"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestCLIExplicitDurableResumeAndColdHistory(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	var output, diagnostics bytes.Buffer
	if err := run(ctx, []string{"--demo", "--data-dir", dir, "--prompt", "first"}, strings.NewReader(""), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	repo, err := session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	list, err := repo.List(ctx, cliOwner, "", 100)
	if err != nil || len(list.Sessions) != 1 {
		t.Fatalf("new native session missing: %+v %v", list, err)
	}
	id := list.Sessions[0].ID
	repo.Close()
	output.Reset()
	diagnostics.Reset()
	if err = run(ctx, []string{"--demo", "--data-dir", dir, "--session", id, "--prompt", "second"}, strings.NewReader(""), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "Tool result: second") {
		t.Fatal("resume did not run local tool")
	}
	var entries []session.RecordEntry
	var after, highWater uint64
	for pages := 0; ; pages++ {
		if pages > 20 {
			t.Fatal("bounded history failed to make progress")
		}
		output.Reset()
		diagnostics.Reset()
		args := []string{"--data-dir", dir, "--session", id, "--history", "--after-record", strconv.FormatUint(after, 10), "--high-water", strconv.FormatUint(highWater, 10)}
		if err := run(ctx, args, strings.NewReader(""), &output, &diagnostics); err != nil {
			t.Fatal(err)
		}
		var page session.RecordPage
		if err := json.Unmarshal(output.Bytes(), &page); err != nil {
			t.Fatal(err)
		}
		if highWater == 0 {
			highWater = page.HighWater
		}
		if page.HighWater != highWater || len(page.Entries) > 20 || page.Next < after || page.Next > highWater {
			t.Fatal("history bounds/snapshot changed", page)
		}
		for _, entry := range page.Entries {
			if entry.Sequence <= after || entry.Sequence > page.Next || entry.Value.URI == "" {
				t.Fatal("bad metadata cursor", entry)
			}
			after = entry.Sequence
			entries = append(entries, entry)
		}
		if strings.Contains(diagnostics.String(), "Native Go session:") || strings.Contains(diagnostics.String(), "Local demo fixture") || strings.Contains(diagnostics.String(), "Live mode:") {
			t.Fatal("cold metadata constructed execution runtime", diagnostics.String())
		}
		if !page.HasMore {
			break
		}
		if page.Next <= after && len(page.Entries) == 0 {
			t.Fatal("history continuation stalled")
		}
		after = page.Next
	}
	// Read individual selected facts only after the cold metadata path. Exact
	// execution counts remain fixed even as independent observation rows grow.
	repo, err = session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	counts := map[abi.RecordKind]int{}
	userTexts, assistantTexts := map[string]bool{}, map[string]bool{}
	modelIntents, modelOutcomes := map[string]bool{}, map[string]bool{}
	for _, entry := range entries {
		if entry.Namespace == "alva.kernel.audit" {
			continue
		}
		_, raw, err := repo.ReadArtifactFull(ctx, cliOwner, id, entry.Value.URI, 1<<20)
		if err != nil {
			t.Fatal(err)
		}
		var record abi.SessionRecord
		if err := json.Unmarshal(raw, &record); err != nil {
			t.Fatal(err)
		}
		if err := record.Validate(); err != nil {
			t.Fatal("invalid actual fact", err)
		}
		if record.Sequence != entry.Sequence || record.ID != entry.ID {
			t.Fatal("metadata did not reference actual fact")
		}
		counts[record.Kind]++
		if record.Message != nil {
			if record.Message.Role == wire.RoleUser {
				userTexts[record.Message.Text()] = true
			}
			if record.Message.Role == wire.RoleAssistant {
				assistantTexts[record.Message.Text()] = true
			}
		}
		if record.ModelIntent != nil {
			modelIntents[record.ModelIntent.RequestID] = true
		}
		if record.ModelOutcome != nil {
			modelOutcomes[record.ModelOutcome.RequestID] = true
			if record.ModelOutcome.Status != abi.OutcomeSucceeded {
				t.Fatal("demo model not actually successful", record.ModelOutcome)
			}
		}
	}
	if counts[abi.RecordConversation] != 8 || counts[abi.RecordModelIntent] != 4 || counts[abi.RecordModelOutcome] != 4 || counts[abi.RecordToolIntent] != 2 || counts[abi.RecordToolOutcome] != 2 || len(modelIntents) != 4 || len(modelOutcomes) != 4 {
		t.Fatal("execution facts missing/replayed behind metadata pagination", counts)
	}
	for request := range modelIntents {
		if !modelOutcomes[request] {
			t.Fatal("unmatched actual model request", request)
		}
	}
	if !userTexts["first"] || !userTexts["second"] || !assistantTexts["[Local demo, not a live model] Tool result: first"] || !assistantTexts["[Local demo, not a live model] Tool result: second"] {
		t.Fatal("resumed conversation changed", userTexts, assistantTexts)
	}
	latest, err := repo.Page(ctx, cliOwner, id, 0, 1, 0)
	if err != nil || latest.HighWater != highWater {
		t.Fatal("metadata path changed durable facts/restored execution", latest, err)
	}
}

func TestAttachmentsAreOriginalBytesAndOnlyReferencesEnterMessages(t *testing.T) {
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(ctx, cliOwner, "fixture", "")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "fixture.log")
	body := []byte(strings.Repeat("private log 日志🙂\n", 1000))
	if err = os.WriteFile(path, body, 0600); err != nil {
		t.Fatal(err)
	}
	messages, err := attachmentMessages(ctx, repo, log, "find failure", []string{path})
	if err != nil {
		t.Fatal(err)
	}
	if len(messages) != 1 || messages[0].Text() != "find failure" || len(messages[0].Content) != 2 || messages[0].Content[1].Kind != wire.ContentReference {
		t.Fatal("attachment bytes entered user prompt")
	}
	reference := messages[0].Content[1].Reference
	if reference.Bytes != int64(len(body)) || !strings.Contains(reference.Processing, "no compression") {
		t.Fatal("processing evidence missing")
	}
	page, err := repo.ReadArtifact(ctx, cliOwner, log.ID(), reference.URI, 0, 4096)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(page.Data, body[:4096]) {
		t.Fatal("original bytes transformed")
	}
}

func TestExplicitStdinKeyOnlyReachesSelectedTransportNeverNativeStore(t *testing.T) {
	const secret = "fixture-stdin-transport-secret"
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer "+secret {
			t.Error("explicit stdin key not used on selected transport")
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(writer, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fixture reply\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	var output, diagnostics bytes.Buffer
	if err := run(ctx, []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--api-key-stdin", "--data-dir", dir, "--prompt", "hello"}, strings.NewReader(secret+"\n"), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "fixture reply") {
		t.Fatal("live fixture response missing")
	}
	repo, err := session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	list, err := repo.List(ctx, cliOwner, "", 100)
	if err != nil || len(list.Sessions) != 1 {
		t.Fatalf("durable source missing: %+v %v", list, err)
	}
	log, err := repo.OpenSession(ctx, cliOwner, list.Sessions[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(records)
	if strings.Contains(string(data), secret) || strings.Contains(output.String()+diagnostics.String(), secret) {
		t.Fatal("stdin transport key leaked into native facts/output")
	}
}

func TestCLIExplicitPersistentMemoryPerformsActualToolAndReopens(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "text/event-stream")
		var delta any
		finish := "stop"
		if calls.Add(1) == 1 {
			arguments, _ := json.Marshal(map[string]string{"key": "remembered-key", "content": "persistent-memory-fixture-marker", "category": "project"})
			delta = map[string]any{"tool_calls": []map[string]any{{"index": 0, "id": "memory-write", "type": "function", "function": map[string]any{"name": "memory_store", "arguments": string(arguments)}}}}
			finish = "tool_calls"
		} else {
			delta = map[string]any{"content": "memory saved by fixture"}
		}
		data, _ := json.Marshal(map[string]any{"choices": []map[string]any{{"index": 0, "delta": delta, "finish_reason": finish}}})
		fmt.Fprintf(writer, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer server.Close()
	directory := filepath.Join(t.TempDir(), "private")
	workspace := t.TempDir()
	var output, diagnostics bytes.Buffer
	err := run(context.Background(), []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--data-dir", directory, "--workspace", workspace, "--memory", "--permission-mode", "accept_edits", "--prompt", "remember explicit fixture"}, strings.NewReader(""), &output, &diagnostics)
	if err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 2 || !strings.Contains(output.String(), "memory saved by fixture") {
		t.Fatalf("memory tool roundtrip not real: %d %s", calls.Load(), output.String())
	}
	store, err := memorysqlite.Open(context.Background(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		t.Fatal(err)
	}
	hash := sha256.Sum256([]byte(canonical))
	backend, err := store.Bind("cli-workspace-" + hex.EncodeToString(hash[:]))
	if err != nil {
		t.Fatal(err)
	}
	entries, err := backend.FTSSearch(context.Background(), "persistent", 10)
	if err != nil || len(entries) != 1 || entries[0].Text != "persistent-memory-fixture-marker" {
		t.Fatalf("configured memory did not persist actual content: %+v %v", entries, err)
	}
	if _, err = os.Stat(filepath.Join(workspace, ".alva-memory")); !os.IsNotExist(err) {
		t.Fatal("memory enabled caused automatic extraction/scan side effects")
	}
}
