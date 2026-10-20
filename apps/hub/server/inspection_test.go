// INPUT: Real isolated native Hub facts and authenticated Inspector SSE/descriptor HTTP
// OUTPUT: Cold declarations, exact durable inspection resume, foreign scope rejection and no replay regressions
// POS: [L7 Hub product] HTTP observes the registered Inspector rather than assuming every engine has Alva storage.
package server

import (
	"bufio"
	"context"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
	"testing"
	"time"

	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestHTTPColdEngineDescriptionAndDurableInspectorStreamResume(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	var descriptions []engine.Description
	if err := json.Unmarshal(body(t, f.request(t, "GET", "/v1/engines", nil, nil), 200), &descriptions); err != nil {
		t.Fatal(err)
	}
	if len(descriptions) != 1 || descriptions[0].ID != "alva" || descriptions[0].Inspection.Level != engine.InspectionFull || !descriptions[0].Inspection.Live || !descriptions[0].Config.SupportsMaxTurns || !descriptions[0].Config.SupportsSystemPrompt || len(descriptions[0].PermissionPlans) != 0 {
		t.Fatal("invented descriptor capabilities", descriptions)
	}
	if sessions, err := f.app.Store.ListSessions(t.Context(), 10); err != nil || len(sessions) != 0 || f.app.Runtime.HasActive() {
		t.Fatal("capability read created/executed a session", err)
	}
	root, foreign := create(t, f, "inspection"), create(t, f, "foreign")
	prefix := "/v1/sessions/" + root.ID
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"one real SDK turn"}`), nil), 202)
	waitIdle(t, f, root.ID)
	detail, err := f.app.Store.Detail(t.Context(), root.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	page, err := f.app.Service.Inspector(t.Context(), root.ID, branch.ID, "", 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	streamPath := prefix + "/inspector-events?branch=" + branch.ID
	for _, suffix := range []string{"&native_id=untrusted", "&child_path=0", "&high_water=1"} {
		body(t, f.request(t, "GET", streamPath+suffix, nil, nil), 400)
	}
	body(t, f.request(t, "GET", "/v1/sessions/"+foreign.ID+"/inspector-events?branch="+branch.ID, nil, nil), 409)
	open := func(after uint64) (*http.Response, context.CancelFunc) {
		t.Helper()
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		req, err := http.NewRequestWithContext(ctx, "GET", f.http.URL+streamPath, nil)
		if err != nil {
			cancel()
			t.Fatal(err)
		}
		req.Header.Set("Authorization", "Bearer "+f.token)
		req.Header.Set("Last-Event-ID", strconv.FormatUint(after, 10))
		resp, err := f.http.Client().Do(req)
		if err != nil {
			cancel()
			t.Fatal(err)
		}
		if resp.StatusCode != 200 {
			cancel()
			t.Fatal("SSE rejected", resp.StatusCode)
		}
		return resp, cancel
	}
	readPage := func(scanner *bufio.Scanner) product.InspectorPage {
		t.Helper()
		var event string
		for scanner.Scan() {
			line := scanner.Text()
			if strings.HasPrefix(line, "event: ") {
				event = strings.TrimPrefix(line, "event: ")
			}
			if event == "inspection" && strings.HasPrefix(line, "data: ") {
				var page product.InspectorPage
				if err := json.Unmarshal([]byte(strings.TrimPrefix(line, "data: ")), &page); err != nil {
					t.Fatal(err)
				}
				return page
			}
		}
		t.Fatal("inspection stream ended before committed page", scanner.Err())
		return product.InspectorPage{}
	}
	resp, cancel := open(0)
	scanner := bufio.NewScanner(resp.Body)
	scanner.Buffer(make([]byte, 4096), 128<<10)
	last := uint64(0)
	for last < page.HighWater {
		observed := readPage(scanner)
		if observed.NativeID != branch.NativeSessionID || len(observed.Entries) == 0 || observed.Entries[0].Sequence != last+1 {
			t.Fatal("stream skipped or changed native authority", observed)
		}
		last = observed.Next
	}
	cancel()
	resp.Body.Close()
	resp, cancel = open(last)
	defer cancel()
	defer resp.Body.Close()
	log, err := f.app.Repository.OpenSession(t.Context(), root.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = log.AppendRecords(t.Context(), []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "fixture", Type: "observer_marker", SchemaVersion: 1, Payload: json.RawMessage(`{"committed":true}`)}}); err != nil {
		t.Fatal(err)
	}
	scanner = bufio.NewScanner(resp.Body)
	scanner.Buffer(make([]byte, 4096), 128<<10)
	resumed := readPage(scanner)
	if resumed.Next != last+1 || len(resumed.Entries) != 1 || resumed.Entries[0].Type != "observer_marker" {
		t.Fatal("inspection resume replayed old facts", resumed)
	}
	if f.app.Runtime.HasActive() {
		t.Fatal("inspection restored Agent execution")
	}
	history, err := f.app.Store.History(t.Context(), root.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs := 0
	for _, entry := range history {
		if entry.Input != "" {
			inputs++
		}
	}
	if inputs != 1 {
		t.Fatal("inspection replayed a Turn", inputs)
	}
}
