// INPUT: Real authenticated HTTP and private product/native stores with local provider streams
// OUTPUT: Team account consent, durable FIFO, cold observation and cross-Lead isolation regressions
// POS: [L7 Hub product] HTTP collaboration acceptance reusing actual product dispatch rather than fake transport outcomes
package server

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestHTTPWorkerAttachmentQueuePinsOnlyUploadedWorkerBranchWithoutExecution(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	lead := create(t, f, "Lead attachment queue")
	team, err := f.app.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w, err := f.app.Service.AddWorker(t.Context(), lead.ID, team.Team.ID, "attached Worker", f.app.Service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	const original = "RAW_WORKER_FILE_MUST_NOT_ENTER_QUEUED_SCOPE"
	var upload product.DraftAttachment
	if json.Unmarshal(body(t, f.request(t, "POST", "/v1/sessions/"+w.SessionID+"/attachments", []byte(original), map[string]string{"X-File-Name": "worker.txt", "Content-Type": "application/octet-stream"}), 201), &upload) != nil {
		t.Fatal("invalid Worker upload")
	}
	base := "/v1/sessions/" + lead.ID + "/teams/" + team.Team.ID + "/workers/" + w.ID
	var task hub.WorkerTask
	if json.Unmarshal(body(t, f.request(t, "POST", base+"/tasks", []byte(`{"scope":"explicit file task","include_draft_attachments":true}`), nil), 201), &task) != nil || !strings.Contains(task.Scope, upload.Reference.URI) || strings.Contains(task.Scope, original) || task.RequiredBranchID != upload.BranchID {
		t.Fatal("queue lost reference/native binding", task)
	}
	if f.app.Runtime.HasActive() || len(f.app.Service.DraftAttachments(w.SessionID)) != 0 {
		t.Fatal("queue executed or retained consumed draft")
	}
	body(t, f.request(t, "PUT", base+"/tasks/"+task.ID, []byte(`{"scope":"not a new attachment send","include_draft_attachments":true}`), nil), 400)
}

func TestHTTPTeamQueueIsDurableNotImplicitExecution(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = io.WriteString(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"actual worker result\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	lead, other := create(t, f, "Lead"), create(t, f, "foreign Lead")
	prefix := "/v1/sessions/" + lead.ID
	var team hub.TeamDetail
	if json.Unmarshal(body(t, f.request(t, "POST", prefix+"/team", []byte(`{}`), nil), 201), &team) != nil || team.Team.ID == "" {
		t.Fatal("missing real Team")
	}
	base := prefix + "/teams/" + team.Team.ID
	newWorker := map[string]any{"name": "worker", "target": f.app.Service.DefaultTarget, "account_and_billing_approved": false}
	data, _ := json.Marshal(newWorker)
	body(t, f.request(t, "POST", base+"/workers", data, nil), 409)
	newWorker["account_and_billing_approved"] = true
	data, _ = json.Marshal(newWorker)
	var worker hub.Worker
	if json.Unmarshal(body(t, f.request(t, "POST", base+"/workers", data, nil), 201), &worker) != nil || worker.SessionID == "" || worker.SessionID == lead.ID {
		t.Fatal("Worker must have an independent product session")
	}
	work := base + "/workers/" + worker.ID
	var first, cancelled hub.WorkerTask
	if json.Unmarshal(body(t, f.request(t, "POST", work+"/tasks", []byte(`{"scope":"original scope","claims":[{"path":".","write":true}]}`), nil), 201), &first) != nil {
		t.Fatal("missing queued task")
	}
	if json.Unmarshal(body(t, f.request(t, "POST", work+"/tasks", []byte(`{"scope":"cancel me"}`), nil), 201), &cancelled) != nil {
		t.Fatal("missing second queued task")
	}
	body(t, f.request(t, "PUT", work+"/tasks/"+first.ID, []byte(`{"scope":"edited scope","claims":[{"path":".","write":false}]}`), nil), 200)
	body(t, f.request(t, "DELETE", work+"/tasks/"+cancelled.ID, nil, nil), 200)
	body(t, f.request(t, "GET", prefix+"/team", nil, nil), 200)
	body(t, f.request(t, "GET", work+"/tasks?limit=32", nil, nil), 200)
	body(t, f.request(t, "GET", work+"/status", nil, nil), 200)
	foreign := strings.Replace(base, lead.ID, other.ID, 1)
	body(t, f.request(t, "GET", foreign, nil, nil), 404)
	body(t, f.request(t, "POST", strings.Replace(work, lead.ID, other.ID, 1)+"/resume", []byte(`{}`), nil), 404)
	if calls.Load() != 0 || f.app.Runtime.HasActive() {
		t.Fatal("creation/enqueue/cold observation silently executed a Worker")
	}
	body(t, f.request(t, "POST", work+"/resume", []byte(`{}`), nil), 202)
	deadline := time.Now().Add(5 * time.Second)
	var page hub.WorkerTaskPage
	for time.Now().Before(deadline) {
		if json.Unmarshal(body(t, f.request(t, "GET", work+"/tasks", nil, nil), 200), &page) != nil {
			t.Fatal("invalid task page")
		}
		if len(page.Tasks) == 2 && page.Tasks[0].Status == engine.Succeeded && !f.app.Runtime.HasActive() {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if len(page.Tasks) != 2 || page.Tasks[0].Status != engine.Succeeded || page.Tasks[1].Status != engine.Cancelled || calls.Load() != 1 {
		t.Fatal("actual FIFO/cancelled settlement failed", page, calls.Load())
	}
	if page.Tasks[0].Scope != "edited scope" || page.Tasks[0].Outcome == nil || page.Tasks[0].Outcome.ResultSequence == 0 || len(page.Tasks[0].Outcome.Verification) != 0 {
		t.Fatal("lost edit/real evidence or inferred verification", page.Tasks[0])
	}
	body(t, f.request(t, "PUT", work+"/tasks/"+first.ID, []byte(`{"scope":"cannot edit terminal"}`), nil), 409)
	body(t, f.request(t, "PUT", work+"/lifecycle", []byte(`{"state":"hibernated"}`), nil), 200)
	body(t, f.request(t, "POST", work+"/resume", []byte(`{}`), nil), 409)
	body(t, f.request(t, "PUT", work+"/lifecycle", []byte(`{"state":"active"}`), nil), 200)
	body(t, f.request(t, "POST", base+"/end", []byte(`{}`), nil), 200)
	if calls.Load() != 1 {
		t.Fatal("terminal observation/lifecycle replayed work")
	}
}
