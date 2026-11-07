// INPUT: Real local provider HTTP, operator task sets and shared developer job ownership.
// OUTPUT: Separate test/evaluation semantics, weighted repeated-trial scores, cold preflight and actual cancellation joins.
// POS: [application developer testing] Fixtures measure implementation behavior, never real model quality.
package developer

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
)

func awaitJob(t *testing.T, s *Service) Snapshot {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		v := s.Snapshot()
		if v.State != "running" {
			return v
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("developer job did not join")
	return Snapshot{}
}
func TestDeveloperServiceColdPlansTestAndEvaluationAreDistinct(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		validReply(w, provider.OpenAIChatKind, data)
	}))
	defer server.Close()
	s, err := NewService([]Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture", APIKey: "private-key"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close(t.Context())
	targets := s.Targets()
	targets[0].ID = "mutated"
	request := Request{Mode: Test, Targets: []string{"configured"}, Repeats: 1}
	invalidNames := request
	invalidNames.ToolNames = []string{"9question"}
	if _, err := s.Plan(invalidNames); err == nil {
		t.Fatal("non-portable question name admitted")
	}
	plan, err := s.Plan(request)
	if err != nil || plan.Kind != Test || len(plan.Scores) != 0 || plan.TotalPlannedCalls != 5 || calls.Load() != 0 || s.Targets()[0].ID != "configured" {
		t.Fatal(plan, err, calls.Load())
	}
	evaluation := request
	evaluation.Mode = Evaluate
	evaluation.Repeats = 3
	plan, err = s.Plan(evaluation)
	if err != nil || plan.Kind != Evaluate || plan.DatasetID != "developer-basics" || len(plan.DatasetSHA256) != 64 || plan.TotalPlannedCalls != 15 || plan.Scores[0].WeightedScore != nil || calls.Load() != 0 {
		t.Fatal(plan, err)
	}
	if err = s.Start(request); err != nil {
		t.Fatal(err)
	}
	result := awaitJob(t, s)
	if result.State != "completed" || result.StartedCalls != 5 || result.Report.Reports[0].Passed != 5 || len(result.Report.Scores) != 0 {
		t.Fatal(result)
	}
	result.Report.Reports[0].Results[0].Status = "mutated"
	if s.Snapshot().Report.Reports[0].Results[0].Status != "passed" {
		t.Fatal("observer mutated owner result")
	}
	data, _ := json.Marshal(s.Snapshot())
	if bytes.Contains(data, []byte("private-key")) || bytes.Contains(data, []byte(server.URL)) {
		t.Fatal("private route leaked")
	}
}
func TestDeveloperEvaluationWeightsRepetitionsAndExitSemantics(t *testing.T) {
	expected := "right"
	dataset := &Dataset{ID: "quality", Version: "v1", Tasks: []Task{{ID: "correct", Prompt: "say right", ExpectedText: &expected, Weight: 3}, {ID: "incorrect", Prompt: "return wrong JSON", ExpectedJSON: json.RawMessage(`{"correct":true}`), Weight: 1}}}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		text := "right"
		if bytes.Contains(data, []byte("return wrong JSON")) {
			text = `{"correct":false}`
		}
		fixtureResponse(w, provider.OpenAIChatKind, "", nil, text)
	}))
	defer server.Close()
	config := fixtureConfig(provider.OpenAIChatKind, server.URL)
	config.ToolNames = nil
	config.Dataset = dataset
	config.Repeats = 2
	file := configFile(t, config)
	var out bytes.Buffer
	err := RunCLI(t.Context(), []string{"eval", "--config", file, "--run"}, strings.NewReader(""), &out, io.Discard)
	var report Report
	if json.Unmarshal(out.Bytes(), &report) != nil || err != nil || calls.Load() != 4 || report.Kind != Evaluate || len(report.Scores) != 1 {
		t.Fatal(report, err, calls.Load())
	}
	score := report.Scores[0]
	if score.WeightedScore == nil || *score.WeightedScore != 75 || score.GradedTrials != 4 || score.ConsistentTaskPasses != 1 || score.TotalTasks != 2 || score.ExecutionErrors != 0 || score.UsageReportedTrials != 4 {
		t.Fatal(score)
	}
	oldHash := report.DatasetSHA256
	dataset.Tasks[0].Weight = 2
	config.Dataset = dataset
	out.Reset()
	report = Report{}
	if err = RunCLI(t.Context(), []string{"eval", "--config", configFile(t, config)}, strings.NewReader(""), &out, io.Discard); err != nil {
		t.Fatal(err)
	}
	if json.Unmarshal(out.Bytes(), &report) != nil || report.DatasetSHA256 == oldHash || report.Scores[0].WeightedScore != nil || calls.Load() != 4 {
		t.Fatal("cold plan scored, called model or lost grading provenance", report)
	}
	if _, _, err := evaluate(t, []string{"--config", file, "--run"}, ""); err == nil || calls.Load() != 4 {
		t.Fatal("task dataset admitted into contract testing")
	}
	dataset.Tasks[0].Weight = 3
	s, err := NewService([]Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close(t.Context())
	if err = s.Start(Request{Mode: Evaluate, Targets: []string{"configured"}, Dataset: dataset, Repeats: 2}); err != nil {
		t.Fatal(err)
	}
	job := awaitJob(t, s)
	if job.State != "completed" || calls.Load() != 8 || job.Report.Scores[0].WeightedScore == nil || *job.Report.Scores[0].WeightedScore != 75 {
		t.Fatal("low quality score was treated as failed execution", job)
	}
}
func TestDeveloperCancelAndCloseKeepActualRequestOwner(t *testing.T) {
	entered, closed := make(chan struct{}, 1), make(chan struct{}, 1)
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		entered <- struct{}{}
		select {
		case <-r.Context().Done():
			closed <- struct{}{}
		case <-release:
		}
	}))
	defer func() { close(release); server.Close() }()
	s, err := NewService([]Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}}})
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Mode: Test, Targets: []string{"configured"}, Repeats: 3}
	if err = s.Start(request); err != nil {
		t.Fatal(err)
	}
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("no request")
	}
	if err = s.Start(request); err == nil {
		t.Fatal("second window started another job")
	}
	if err = s.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	select {
	case <-closed:
	case <-time.After(3 * time.Second):
		t.Fatal("Close did not cancel actual HTTP request")
	}
	v := s.Snapshot()
	if v.State != "canceled" || v.StartedCalls != 1 || v.Report.Reports[0].AttemptedCalls != 1 || v.Report.Reports[0].PlannedCalls != 15 {
		t.Fatal(v)
	}
	if err = s.Start(request); err == nil {
		t.Fatal("closed service restarted")
	}
	if err = s.Cancel(context.Background()); err != nil {
		t.Fatal(err)
	}
}

type delayedTransport func(*http.Request) (*http.Response, error)

func (f delayedTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestDeveloperCloseTimeoutRetainsRunningOwnerUntilTransportReturns(t *testing.T) {
	entered, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	releaseRequest := func() { once.Do(func() { close(release) }) }
	defer releaseRequest()
	client := &http.Client{Transport: delayedTransport(func(r *http.Request) (*http.Response, error) {
		close(entered)
		<-release
		return nil, r.Context().Err()
	})}
	s, err := NewService([]Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1/model", Model: "fixture", HTTPClient: client}}})
	if err != nil {
		t.Fatal(err)
	}
	request := Request{Mode: Test, Targets: []string{"configured"}}
	if err = s.Start(request); err != nil {
		t.Fatal(err)
	}
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("transport not entered")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err = s.Close(ctx); err == nil || s.Snapshot().State != "running" {
		t.Fatal("Close hid unjoined request", err, s.Snapshot())
	}
	if err = s.Start(request); err == nil {
		t.Fatal("retained owner admitted new work")
	}
	releaseRequest()
	if err = s.Close(t.Context()); err != nil || s.Snapshot().State != "canceled" {
		t.Fatal(err, s.Snapshot())
	}
}
