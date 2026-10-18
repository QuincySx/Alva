// INPUT: context, encoding/json, errors, http/httptest, os, filepath, strings, testing, time and protocol/browser.
// OUTPUT: Actual isolated Chrome CDP catalog, source/encoding, authorization admission and real cleanup regressions.
// POS: [platform / native execution host] Explicit installed Chrome is used only with a fresh private profile and local fixture pages, never an existing browser/profile.
package browser

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	protocol "github.com/QuincySx/Alva/integration/protocol/browser"
)

const fixtureChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

func TestActualChromeCompleteCatalogAndPrivateCleanup(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable; no discovery or installation")
	}
	t.Setenv("ALVA_HOST_SECRET", "must-not-inherit")
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		switch r.URL.Path {
		case "/redirect":
			http.Redirect(w, r, "/fixture", http.StatusFound)
		default:
			w.Write([]byte(`<html><head><title>Fixture🙂</title></head><body style="height:2000px"><nav>navigation-noise</nav><article id="story" style="background:#eee;width:400px;height:500px">` + strings.Repeat("article-日志🙂 ", 100) + `</article><input id="input"><button id="button" onclick="document.querySelector('#input').value+='clicked';document.body.dataset.value=document.querySelector('#input').value">Go</button></body></html>`))
		}
	}))
	defer server.Close()
	temp := t.TempDir()
	os.Chmod(temp, 0700)
	var frames int
	m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { frames++; return nil }})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := m.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	starts := 0
	status, err := m.Start(ctx, "fixture", func(context.Context, Launch) error {
		starts++
		entries, _ := os.ReadDir(temp)
		if len(entries) != 1 {
			t.Error("missing unique private allocation")
		}
		for _, status := range m.Status() {
			if status.Running {
				t.Error("browser already running before durable admission")
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if !status.Running || starts != 1 {
		t.Fatal(status, starts)
	}
	client, err := m.Client("fixture")
	if err != nil {
		t.Fatal(err)
	}
	info, err := client.Navigate(ctx, server.URL+"/redirect")
	if err != nil || info.URL != server.URL+"/fixture" || info.Title != "Fixture🙂" {
		t.Fatal(info, err)
	}
	if err = client.Action(ctx, protocol.Action{Kind: "type", Selector: "#input", Text: "typed🙂"}); err != nil {
		t.Fatal(err)
	}
	if err = client.Action(ctx, protocol.Action{Kind: "click", Selector: "#button"}); err != nil {
		t.Fatal(err)
	}
	if err = client.Action(ctx, protocol.Action{Kind: "press", Key: "Tab"}); err != nil {
		t.Fatal(err)
	}
	if err = client.Action(ctx, protocol.Action{Kind: "scroll", Direction: "down", Amount: 100}); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"text", "html", "readability"} {
		snapshot, err := client.Snapshot(ctx, mode, "")
		if err != nil || !strings.Contains(snapshot.Content, "article-日志🙂") || snapshot.Processing == "" {
			t.Fatal(snapshot, err)
		}
		if mode == "readability" && strings.Contains(snapshot.Content, "navigation-noise") {
			t.Fatal("readability did not perform stated noise processing")
		}
	}
	snapshot, err := client.Snapshot(ctx, "html", "#input")
	if err != nil || !strings.Contains(snapshot.Content, `id="input"`) {
		t.Fatal(snapshot, err)
	}
	snapshot, err = client.Snapshot(ctx, "html", "body")
	if err != nil || !strings.Contains(snapshot.Content, `data-value="typed🙂clicked"`) {
		t.Fatal("actual type/click input did not reach page", snapshot, err)
	}
	for _, o := range []protocol.ScreenshotOptions{{Format: "png"}, {Format: "jpeg", Quality: 60, FullPage: true}, {Format: "png", Selector: "#story"}} {
		shot, err := client.Screenshot(ctx, o)
		if err != nil || len(shot.Data) == 0 || shot.Width == 0 || shot.Format != o.Format || shot.Processing == "" {
			t.Fatal(shot, err)
		}
	}
	tabs, err := client.Tabs(ctx)
	if err != nil || len(tabs) != 2 {
		t.Fatal(tabs, err)
	}
	if _, err = client.Snapshot(ctx, "html", `#input');throw new Error('injected');('`); err == nil {
		t.Fatal("invalid selector accepted")
	}
	if _, err = client.Navigate(ctx, "file:///etc/passwd"); err == nil {
		t.Fatal("file URL accepted")
	}
	status, err = m.Stop(ctx, "fixture")
	if err != nil || !status.Reaped || !status.ProfileRemoved {
		t.Fatal(status, err)
	}
	entries, err := os.ReadDir(temp)
	if err != nil || len(entries) != 0 {
		t.Fatal("owned private profile not removed", entries, err)
	}
	if frames == 0 {
		t.Fatal("browser process evidence not observed")
	}
}

func TestBrowserFailedPriorAdmissionNeverStartsAndRemovesOnlyOwnAllocation(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable")
	}
	temp := t.TempDir()
	os.Chmod(temp, 0700)
	marker := filepath.Join(temp, "keep-user-file")
	os.WriteFile(marker, []byte("kept"), 0600)
	m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { t.Error("unapproved process emitted IO"); return nil }})
	if err != nil {
		t.Fatal(err)
	}
	failure := errors.New("durable risk report failed")
	_, err = m.Start(context.Background(), "fixture", func(context.Context, Launch) error { return failure })
	if !errors.Is(err, failure) || len(m.Status()) != 0 {
		t.Fatal(err, m.Status())
	}
	entries, _ := os.ReadDir(temp)
	if len(entries) != 1 || entries[0].Name() != "keep-user-file" {
		t.Fatal("unstarted cleanup erased unrelated data", entries)
	}
	if err = m.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestBrowserEndpointAndPrivateConfigurationRejectDiscovery(t *testing.T) {
	for _, endpoint := range []string{"http://127.0.0.1:9222/", "ws://localhost:9222/devtools/browser/id", "ws://192.0.2.1:9222/devtools/browser/id", "ws://127.0.0.1:9222/", "ws://a:b@127.0.0.1:9222/devtools/browser/id"} {
		if protocol.ValidateEndpoint(endpoint) == nil {
			t.Fatal("non-owned endpoint accepted", endpoint)
		}
	}
	if err := protocol.ValidateEndpoint("ws://127.0.0.1:12345/devtools/browser/explicit"); err != nil {
		t.Fatal(err)
	}
	if _, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: t.TempDir(), Env: map[string]string{"HOME": "/existing-user"}, Observe: func(context.Context, IOEvent) error { return nil }}); err == nil {
		t.Fatal("existing home override accepted")
	}
}

func TestCloseDuringPriorCallbackIsBoundedAndNeverLaunchesAfterCancellation(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable")
	}
	temp := t.TempDir()
	os.Chmod(temp, 0700)
	m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { t.Error("canceled admission launched browser"); return nil }})
	if err != nil {
		t.Fatal(err)
	}
	entered, release := make(chan struct{}), make(chan struct{})
	done := make(chan error, 1)
	go func() {
		_, err := m.Start(context.Background(), "pending", func(context.Context, Launch) error { _ = m.Status(); close(entered); <-release; return nil })
		done <- err
	}()
	<-entered
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	started := time.Now()
	if err = m.Close(ctx); !errors.Is(err, context.DeadlineExceeded) || time.Since(started) > time.Second {
		t.Fatal("Close blocked on start mutex or claimed cleanup", err)
	}
	if m.CleanupComplete() {
		t.Fatal("pending callback claimed cleanup complete")
	}
	close(release)
	select {
	case err = <-done:
		if err == nil {
			t.Fatal("late prior callback launched after Close")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("canceled startup did not settle")
	}
	if err = m.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !m.CleanupComplete() {
		t.Fatal("finished late admission retained fake active resource")
	}
	entries, _ := os.ReadDir(temp)
	if len(entries) != 0 {
		t.Fatal("canceled pending profile leaked", entries)
	}
}

func TestSeparateOwnedInstancesNeverShareCookiesAndLimitsAreExplicit(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable")
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		if r.URL.Path == "/set" {
			http.SetCookie(w, &http.Cookie{Name: "private_cookie", Value: "one-only", Path: "/"})
		}
		w.Write([]byte("<html><title>cookies</title><body>cookie-source:" + r.Header.Get("Cookie") + "</body></html>"))
	}))
	defer server.Close()
	temp := t.TempDir()
	os.Chmod(temp, 0700)
	m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { return nil }})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := m.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	admit := func(context.Context, Launch) error { return nil }
	for _, id := range []string{"one", "two"} {
		if _, err = m.Start(ctx, id, admit); err != nil {
			t.Fatal(err)
		}
	}
	if _, err = m.Start(ctx, "one", admit); err == nil {
		t.Fatal("duplicate instance launched")
	}
	one, _ := m.Client("one")
	two, _ := m.Client("two")
	if _, err = one.Navigate(ctx, server.URL+"/set"); err != nil {
		t.Fatal(err)
	}
	if _, err = one.Navigate(ctx, server.URL+"/read"); err != nil {
		t.Fatal(err)
	}
	if _, err = two.Navigate(ctx, server.URL+"/read"); err != nil {
		t.Fatal(err)
	}
	first, err := one.Snapshot(ctx, "text", "")
	if err != nil || !strings.Contains(first.Content, "one-only") {
		t.Fatal("fixture cookie never set", first, err)
	}
	second, err := two.Snapshot(ctx, "text", "")
	if err != nil || strings.Contains(second.Content, "one-only") {
		t.Fatal("independent profile leaked cookie", second, err)
	}
	if _, err = m.Stop(ctx, "one"); err != nil {
		t.Fatal(err)
	}
	statuses := m.Status()
	if len(statuses) != 1 || statuses[0].ID != "two" || !statuses[0].Running {
		t.Fatal("stopping one affected sibling browser", statuses)
	}
	if err = m.Close(ctx); err != nil || !m.CleanupComplete() {
		t.Fatal("independent profiles not really cleaned", err)
	}
}

func TestObserverClosedErrorNeverMasqueradesAsPipeClosure(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable")
	}
	for _, cause := range []error{os.ErrClosed, errors.Join(errors.New("private disk error"), os.ErrClosed)} {
		temp := t.TempDir()
		os.Chmod(temp, 0700)
		m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { return cause }})
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		status, err := m.Start(ctx, "failure", func(context.Context, Launch) error { return nil })
		cancel()
		var observer *ObserverError
		if err == nil || !errors.As(err, &observer) || !errors.Is(err, os.ErrClosed) || status.Running || !status.Reaped || strings.Contains(err.Error(), "private disk error") {
			t.Fatal("observer failure lost/leaked or running fabricated", status, err)
		}
		if err = m.Close(context.Background()); err != nil {
			t.Fatal(err)
		}
		entries, _ := os.ReadDir(temp)
		if len(entries) != 0 {
			t.Fatal("failed observer left profile", entries)
		}
	}
}

func TestProtocolCloseCancelsActualInFlightOperationBeforeWaitingForMutex(t *testing.T) {
	if _, err := os.Stat(fixtureChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable")
	}
	entered := make(chan struct{}, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case entered <- struct{}{}:
		default:
		}
		<-r.Context().Done()
	}))
	t.Cleanup(server.Close)
	temp := t.TempDir()
	os.Chmod(temp, 0700)
	m, err := New(Config{Executable: fixtureChrome, Cwd: t.TempDir(), TempRoot: temp, Headless: true, Observe: func(context.Context, IOEvent) error { return nil }})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := m.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if _, err = m.Start(ctx, "active", func(context.Context, Launch) error { return nil }); err != nil {
		t.Fatal(err)
	}
	client, _ := m.Client("active")
	done := make(chan error, 1)
	go func() { _, err := client.Navigate(context.Background(), server.URL); done <- err }()
	select {
	case <-entered:
	case <-time.After(5 * time.Second):
		t.Fatal("actual navigation not active")
	}
	closed := make(chan struct{})
	go func() { client.Close(); close(closed) }()
	select {
	case <-closed:
	case <-time.After(5 * time.Second):
		t.Fatal("protocol Close waits on active-operation mutex before cancellation")
	}
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("unfinished navigation claimed success")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("protocol operation did not really exit")
	}
}
