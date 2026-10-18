// INPUT: context, encoding/json, errors, io, net/http, net/http/httptest, os, path/filepath, strings, sync/atomic, testing, wazero experimental/sys, sandbox/abi
// OUTPUT: Real fetch/escalation and descriptor-root race/readonly regressions
// POS: [platform / WASM host or guest] Tests independent host policy boundaries with actual guest imports and local-only servers.
package host

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/sdk/sandbox/abi"
	expsys "github.com/tetratelabs/wazero/experimental/sys"
)

func TestWASIFetchValidatesEveryRedirectAndNeverUsesAmbientProxy(t *testing.T) {
	var escaped atomic.Int32
	forbidden := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { escaped.Add(1); _, _ = io.WriteString(w, "outside") }))
	defer forbidden.Close()
	allowed := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/ok":
			_, _ = io.WriteString(w, "allowed bytes")
		case "/redirect":
			http.Redirect(w, r, strings.Replace(forbidden.URL, "127.0.0.1", "localhost", 1), http.StatusFound)
		default:
			http.Error(w, "missing", 404)
		}
	}))
	defer allowed.Close()
	proxyHits := 0
	proxy := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { proxyHits++; _, _ = io.WriteString(w, "proxy forbidden") }))
	defer proxy.Close()
	t.Setenv("HTTP_PROXY", proxy.URL)
	t.Setenv("HTTPS_PROXY", proxy.URL)
	for _, test := range []struct {
		path    string
		allow   bool
		success bool
	}{{"/ok", true, true}, {"/ok", false, false}, {"/redirect", true, false}} {
		request := requestFor(t, true, "fetch")
		request.Context.Task = allowed.URL + test.path
		if test.allow {
			request.AllowedDomains = []string{"127.0.0.1"}
		}
		outcome, err := runFixture(context.Background(), request, Capabilities{})
		if err != nil || outcome.ExitCode != 0 {
			t.Fatal("fetch guest failed", string(outcome.Stderr), err)
		}
		var result abi.FetchResult
		if err := json.Unmarshal(outcome.Stdout, &result); err != nil {
			t.Fatal(err)
		}
		if (result.Response != nil) != test.success {
			t.Fatal("fetch policy result", string(outcome.Stdout))
		}
		if test.success && string(result.Response.Body) != "allowed bytes" {
			t.Fatal("actual network body missing")
		}
	}
	if escaped.Load() != 0 || proxyHits != 0 {
		t.Fatal("forbidden redirect/proxy reached", escaped.Load(), proxyHits)
	}
	for _, header := range []string{"Host", "Content-Length", "Connection", "Transfer-Encoding"} {
		_, err := executeFetch(context.Background(), []string{"127.0.0.1"}, abi.FetchRequest{Version: abi.Version, Method: "GET", URL: allowed.URL, Headers: []abi.Header{{Name: header, Value: "forbidden"}}})
		if err == nil {
			t.Fatal("unsafe header accepted", header)
		}
	}
}

func TestWASIEscalationRequiresOptInApprovalAndRealGrantedDirectory(t *testing.T) {
	for _, test := range []struct {
		name                          string
		optIn, policy, approve, valid bool
		calls                         int
	}{{"default-deny", false, true, true, true, 0}, {"guest-deny", true, false, true, true, 0}, {"HITL-deny", true, true, false, true, 0}, {"ungranted-cwd", true, true, true, false, 0}, {"approved", true, true, true, true, 1}} {
		t.Run(test.name, func(t *testing.T) {
			request := requestFor(t, true, "escalate")
			request.Context.Policy.AllowEscalationRequest = test.policy
			request.Context.Task = "/work"
			if !test.valid {
				request.Context.Task = "/outside"
			}
			calls := 0
			caps := Capabilities{AllowUnsandboxed: test.optIn, Approval: ApprovalFunc(func(_ context.Context, target EscalationTarget) (bool, error) {
				if target.JobID != request.JobID || target.Request.Cwd != "/work" {
					t.Error("wrong bound target")
				}
				if _, err := target.Directory.Stat("in.txt"); err != nil {
					t.Error("not an actual pinned directory", err)
				}
				return test.approve, nil
			}), Executor: ExecutorFunc(func(_ context.Context, target EscalationTarget) (abi.EscalationResponse, error) {
				calls++
				if target.Request.Command != "explicit-command" {
					t.Error("input changed")
				}
				return abi.EscalationResponse{Stdout: "approved native callback", ExitCode: 7}, nil
			})}
			outcome, err := runFixture(context.Background(), request, caps)
			if err != nil || outcome.ExitCode != 0 || calls != test.calls {
				t.Fatal("escalation gate mismatch", string(outcome.Stdout), string(outcome.Stderr), calls, err)
			}
			var result abi.EscalationResult
			if err := json.Unmarshal(outcome.Stdout, &result); err != nil {
				t.Fatal(err)
			}
			if (result.Response != nil) != (test.calls == 1) {
				t.Fatal("false execution claim", string(outcome.Stdout))
			}
		})
	}
}

func TestRootedWASIPathRaceCannotReadOutsideGrant(t *testing.T) {
	directory := t.TempDir()
	outside := t.TempDir()
	if err := os.WriteFile(filepath.Join(outside, "secret"), []byte("outside-marker"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(directory, "inside"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "inside", "secret"), []byte("allowed"), 0600); err != nil {
		t.Fatal(err)
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	filesystem := &rootedFS{root: root, budget: &fsBudget{}}
	stop := make(chan struct{})
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			select {
			case <-stop:
				return
			default:
			}
			_ = os.Remove(filepath.Join(directory, "race"))
			_ = os.Symlink(outside, filepath.Join(directory, "race"))
			_ = os.Remove(filepath.Join(directory, "race"))
			_ = os.Symlink("inside", filepath.Join(directory, "race"))
		}
	}()
	defer func() { close(stop); <-done }()
	for range 500 {
		file, errno := filesystem.OpenFile("race/secret", expsys.O_RDONLY, 0)
		if errno != 0 {
			continue
		}
		data := make([]byte, 64)
		n, e := file.Read(data)
		file.Close()
		if e == 0 && string(data[:n]) != "allowed" {
			t.Fatal("descriptor root escaped under symlink race", string(data[:n]))
		}
	}
	if _, errno := filesystem.OpenFile("../secret", expsys.O_RDONLY, 0); errno == 0 {
		t.Fatal("parent traversal accepted")
	}
	filesystem.readOnly = true
	if _, errno := filesystem.OpenFile("inside/new", expsys.O_WRONLY|expsys.O_CREAT, 0600); errno != expsys.EROFS {
		t.Fatal("readonly open not host enforced", errno)
	}
	if errno := filesystem.Rename("inside/secret", "renamed"); errno != expsys.EROFS {
		t.Fatal("readonly mutation not host enforced")
	}
}

func TestWASIHostApprovalFailureIsFatalNotBusinessDenial(t *testing.T) {
	request := requestFor(t, true, "escalate")
	request.Context.Task = "/work"
	request.Context.Policy.AllowEscalationRequest = true
	disk := errors.New("approval storage failed")
	calls := 0
	_, err := runFixture(context.Background(), request, Capabilities{AllowUnsandboxed: true, Approval: ApprovalFunc(func(context.Context, EscalationTarget) (bool, error) { return false, disk }), Executor: ExecutorFunc(func(context.Context, EscalationTarget) (abi.EscalationResponse, error) {
		calls++
		return abi.EscalationResponse{}, nil
	})})
	if !errors.Is(err, disk) || calls != 0 {
		t.Fatal("approval infrastructure error swallowed", err, calls)
	}
}
