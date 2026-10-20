// INPUT: context, encoding/json, errors, io/http/httptest, filepath, strings, testing/time, SDK/security and native session facts.
// OUTPUT: Actual HTTP/source/search/HITL/DNS/redirect/overflow/cancellation and no-inline-context regressions.
// POS: [L4 extension] Local explicit servers exercise real web behavior without external services, credentials or ambient proxies.
package web

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	securityplugin "github.com/QuincySx/Alva/extension/security"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(c context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return f(c, r, e)
}

type fixture struct {
	repo    storage.Repository
	log     storage.Session
	cwd     string
	guard   *security.Guard
	reports atomic.Int32
}

func newFixture(t *testing.T, allow bool) *fixture {
	t.Helper()
	repo, err := session.Open(context.Background(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(context.Background(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{repo: repo, log: log, cwd: t.TempDir()}
	f.guard, err = security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: allow, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return f
}
func (f *fixture) plugin(t *testing.T, server string, search bool) *Plugin {
	t.Helper()
	c := Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, ApprovedNetworkAccess: true, ReadURLs: true, Network: NetworkPolicy{AllowedPrivateOrigins: []string{server}}, Report: func(context.Context, Activity) error { f.reports.Add(1); return nil }}
	if search {
		c.Search = &SearchConfig{Kind: "duckduckgo_instant", Endpoint: server}
	}
	p, err := New(context.Background(), c)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		if err := p.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return p
}
func direct(t *testing.T, f *fixture, p *Plugin, name, args string) (wire.ToolResult, error) {
	t.Helper()
	state, err := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, p.Tools())
	if err != nil {
		t.Fatal(err)
	}
	if err = p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "run", State: state, Workspace: f.cwd}); err != nil {
		t.Fatal(err)
	}
	defer p.OnRunEnd(context.Background(), abi.RuntimeContext{RunID: "run"}, nil)
	for _, tool := range p.Tools() {
		if tool.Name() == name {
			return tool.Execute(context.Background(), abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: tool}, wire.ToolCall{ID: "call", Name: name, Arguments: json.RawMessage(args)})
		}
	}
	t.Fatal("missing tool")
	return wire.ToolResult{}, nil
}
func readRef(t *testing.T, f *fixture, ref wire.Reference) string {
	t.Helper()
	_, b, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", ref.URI, 20<<20)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func TestActualHTMLReferencesAndSDKRequestNotRawBody(t *testing.T) {
	var count atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		count.Add(1)
		if r.Header.Get("Cookie") != "" || r.Header.Get("Authorization") != "" {
			t.Error("ambient credentials")
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		io.WriteString(w, "<h1>Evidence🙂</h1><script>not-visible</script><p>"+strings.Repeat("source-only-marker ", 2000)+"</p>")
	}))
	defer server.Close()
	f := newFixture(t, true)
	p := f.plugin(t, server.URL, false)
	t.Setenv("HTTP_PROXY", "http://127.0.0.1:1")
	turn := 0
	m := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			args, _ := json.Marshal(map[string]string{"url": server.URL})
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "read_url", Arguments: args}}); err != nil {
				return err
			}
		} else {
			for _, msg := range r.Messages {
				if strings.Contains(msg.Text(), "source-only-marker") {
					return errors.New("body leaked to model")
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	a, err := core.NewBuilder(m, f.log).Workspace(f.cwd).Plugin(p).Plugin(securityplugin.NewSecurityPlugin(f.guard, p.Tools())).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "read explicit URL")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if count.Load() != 1 || f.reports.Load() != 1 {
		t.Fatal("not one actual request/risk report")
	}
	artifacts, err := f.repo.PageArtifacts(context.Background(), "owner", "native", "web-html-text", "", 0, 0, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(artifacts.Entries) != 1 {
		t.Fatalf("missing derived text: %+v", artifacts)
	}
	text := readRef(t, f, artifacts.Entries[0].Info.Reference())
	if !strings.Contains(text, "Evidence🙂") || strings.Contains(text, "not-visible") {
		t.Fatal("wrong actual HTML conversion")
	}
}
func TestActualSearchAndNoProviderIsNoTool(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("q") != "explicit query" {
			t.Error("query not sent")
		}
		w.Header().Set("Content-Type", "application/json")
		io.WriteString(w, `{"AbstractText":"actual abstract","AbstractURL":"https://allowed.example/a","Heading":"A","api_key":"fixture-secret","RelatedTopics":[{"Topics":[{"Text":"B","FirstURL":"https://child.allowed.example/b"},{"Text":"C","FirstURL":"https://blocked.example/c"}]}]}`)
	}))
	defer server.Close()
	f := newFixture(t, true)
	p := f.plugin(t, server.URL, true)
	result, err := direct(t, f, p, "internet_search", `{"query":"explicit query","allowed_domains":["allowed.example"]}`)
	if err != nil {
		t.Fatal(err)
	}
	var value receipt
	if err = json.Unmarshal([]byte(result.Content[0].Text), &value); err != nil {
		t.Fatal(err)
	}
	if value.Results != 2 || value.Derived == nil {
		t.Fatalf("not real filtered results: %+v", value)
	}
	if strings.Contains(readRef(t, f, value.Source), "fixture-secret") {
		t.Fatal("JSON credential persisted")
	}
	if !strings.Contains(readRef(t, f, *value.Derived), "child.allowed.example") {
		t.Fatal("nested result missing")
	}
	q := f.plugin(t, server.URL, false)
	if len(q.Tools()) != 1 || q.Tools()[0].Name() != "read_url" {
		t.Fatal("fake unconfigured search")
	}
	if _, err = instantAnswers([]byte(`{}`), 5, nil, nil); err == nil {
		t.Fatal("wrong provider schema treated as empty search")
	}
}
func TestPrivateDestinationRedirectOverflowAndCancel(t *testing.T) {
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, "must-not-reach") }))
	defer target.Close()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/redirect":
			http.Redirect(w, r, target.URL, http.StatusFound)
		case "/large":
			io.WriteString(w, strings.Repeat("x", maxBody+1))
		case "/wait":
			<-r.Context().Done()
		default:
			io.WriteString(w, "ok")
		}
	}))
	defer server.Close()
	if _, err := fetch(context.Background(), server.URL, NetworkPolicy{}); err == nil {
		t.Fatal("unapproved private URL allowed")
	}
	policy := NetworkPolicy{AllowedPrivateOrigins: []string{server.URL}}
	if _, err := fetch(context.Background(), server.URL+"/redirect", policy); err == nil {
		t.Fatal("private redirect allowed")
	}
	if _, err := fetch(context.Background(), server.URL+"/large", policy); err == nil {
		t.Fatal("oversized successful prefix")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	if _, err := fetch(ctx, server.URL+"/wait", policy); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("cancel not propagated: %v", err)
	}
}
func TestRiskFailureOrDeniedApprovalNeverRequests(t *testing.T) {
	var count atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { count.Add(1) }))
	defer server.Close()
	for _, allow := range []bool{false, true} {
		t.Run(map[bool]string{false: "deny", true: "risk-fail"}[allow], func(t *testing.T) {
			f := newFixture(t, allow)
			p := f.plugin(t, server.URL, false)
			if allow {
				p.config.Report = func(context.Context, Activity) error { return errors.New("durable activity unavailable") }
			}
			args, _ := json.Marshal(map[string]string{"url": server.URL})
			if _, err := direct(t, f, p, "read_url", string(args)); err == nil {
				t.Fatal("admission failure succeeded")
			}
		})
	}
	if count.Load() != 0 {
		t.Fatal("network preceded admission")
	}
}

func TestHTMLSkippedContainersRemainSkippedAcrossNestedTags(t *testing.T) {
	for _, source := range []string{`<template><script>hidden()</script>template-private</template><p>visible</p>`, `<template><template>nested-private</template>outer-private</template><p>visible</p>`, `<noscript><style>.private{}</style>fallback-private</noscript><p>visible</p>`} {
		value, err := htmlText([]byte(source))
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(value), "private") || !strings.Contains(string(value), "visible") {
			t.Fatalf("skip declaration violated: %s", value)
		}
	}
}
