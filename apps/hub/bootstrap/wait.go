// INPUT: Actual native log/lease, explicit bootstrap recipe and optional durable Controller.
// OUTPUT: Durable runner, immutable execution recipe check and post-cleanup waiting receipt.
// POS: [L7 composition] Reuses ordinary Agent/Plugin/Guard; no second execution runtime or kernel business state.
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/extension/compatibility"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"path/filepath"
	"time"
)

const waitRecipeNamespace = "alva.hub.waits"

type waitRecipe struct {
	Version int    `json:"version"`
	SHA256  string `json:"sha256"`
}

func compatibilityFingerprint(ctx context.Context, workspace, file string) (hash string, err error) {
	if file == "" {
		return "", nil
	}
	snapshot, err := compatibility.OpenFile(ctx, workspace, file)
	if err != nil {
		return "", err
	}
	defer func() { err = errors.Join(err, snapshot.Close(context.WithoutCancel(ctx))) }()
	return snapshot.Fingerprint(), nil
}

func hubWaitRecipe(c Config, workspace string, target hub.Target, settings hub.Settings, compatibilitySHA string) (string, error) {
	cwd, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return "", err
	}
	target.AutomaticAuthorized, target.RequiredCapabilities = false, nil
	settings.SkillArchives = nil // Observation receipts do not change the execution recipe.
	data, err := json.Marshal(struct {
		Workspace       string
		Target          hub.Target
		Settings        hub.Settings
		Demo, Images    bool
		Provider        string
		Model, Endpoint string
		Headers         map[string]string
		Compatibility   string `json:",omitempty"`
	}{cwd, target, settings, c.Demo, c.ImageCapable, string(c.ProviderKind), c.Provider.Model, c.Provider.Endpoint, c.Provider.Headers, compatibilitySHA})
	if err != nil {
		return "", err
	}
	hash := sha256.Sum256(append([]byte("alva.hub.waits.recipe.v1\x00"), data...))
	return hex.EncodeToString(hash[:]), nil
}
func checkHubWaitRecipe(ctx context.Context, repository storage.Repository, owner string, log storage.Session, hash string, resume bool) error {
	entry, found, err := repository.LatestRecord(ctx, owner, log.ID(), waitRecipeNamespace, "recipe")
	if err != nil {
		return err
	}
	if found {
		if entry.SchemaVersion != 1 {
			return errors.New("unsupported durable recipe schema")
		}
		_, data, e := repository.ReadArtifactFull(ctx, owner, log.ID(), entry.Value.URI, 4096)
		if e != nil {
			return e
		}
		var r abi.SessionRecord
		if e = json.Unmarshal(data, &r); e != nil {
			return e
		}
		if e = r.Validate(); e != nil {
			return e
		}
		if r.ID != entry.ID || r.Sequence != entry.Sequence || r.Namespace != waitRecipeNamespace || r.Type != "recipe" || r.SchemaVersion != 1 {
			return errors.New("durable recipe metadata mismatch")
		}
		var previous waitRecipe
		if e = json.Unmarshal(r.Payload, &previous); e != nil {
			return e
		}
		if digest, e := hex.DecodeString(previous.SHA256); e != nil || previous.Version != 1 || len(digest) != sha256.Size {
			return errors.New("unsupported durable recipe")
		}
		if previous.SHA256 == hash {
			return nil
		}
	}
	if resume {
		return errors.New("durable continuation recipe changed; inspect and abandon before new work")
	}
	data, _ := json.Marshal(waitRecipe{1, hash})
	_, err = log.AppendExtensionSnapshot(ctx, waitRecipeNamespace, "recipe", entry.Sequence, data)
	return err
}

type claimedWaitBackend struct {
	durable.Backend
	owner *ownedRunner
}

func (b claimedWaitBackend) Claim(ctx context.Context) (func(), error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if b.owner.release == nil {
		return nil, errors.New("actual native lease required before durable execution")
	}
	return func() {}, nil
}

type durableRunner struct {
	agent      *core.Agent
	controller *durable.Controller
	backend    *journal.Backend
	revision   uint64
	checkpoint *engine.WaitCheckpoint
}

func (r *durableRunner) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	var out durable.Outcome
	var err error
	if r.revision != 0 {
		s, e := r.backend.Load(ctx)
		if e != nil {
			return e
		}
		if s.Revision != r.revision {
			return durable.ErrConflict
		}
		out, err = r.controller.Resume(ctx, r.agent, emit)
	} else {
		out, err = r.controller.Start(ctx, r.agent, input, emit)
	}
	if err != nil {
		return err
	}
	if out.Suspended {
		s, e := r.backend.Load(ctx)
		if e != nil {
			return e
		}
		if s.State.Status != durable.Waiting || len(s.State.Requests) == 0 {
			return errors.New("durable suspension lacks committed checkpoint")
		}
		expires := s.State.Requests[0].ExpiresAt
		for _, request := range s.State.Requests {
			if request.ExpiresAt.Before(expires) {
				expires = request.ExpiresAt
			}
		}
		r.checkpoint = &engine.WaitCheckpoint{Revision: s.Revision, Requests: len(s.State.Requests), ExpiresAt: expires.UnixMilli()}
	}
	return nil
}
func (r *durableRunner) WaitCheckpoint() *engine.WaitCheckpoint {
	if r.checkpoint == nil {
		return nil
	}
	copy := *r.checkpoint
	return &copy
}
func openHubWaits(ctx context.Context, repo storage.Repository, owner string, log storage.Session, settings hub.Settings, request engine.Request, owned *ownedRunner, c Config, compatibilitySHA string) (*journal.Backend, *durable.Controller, error) {
	_, found, err := repo.LatestRecord(ctx, owner, log.ID(), durable.Namespace, durable.SnapshotType)
	if err != nil {
		return nil, nil, err
	}
	if !settings.DurableWaits && !found {
		if request.WaitRevision != 0 {
			return nil, nil, engine.ErrUnsupported
		}
		return nil, nil, nil
	}
	backend, err := journal.Open(ctx, repo, owner, log.ID())
	if err != nil {
		return nil, nil, err
	}
	snapshot, err := backend.Load(ctx)
	if err != nil {
		return nil, nil, err
	}
	if snapshot.State.Status == durable.Running || snapshot.State.Status == durable.Interrupted {
		return nil, nil, durable.ErrInterrupted
	}
	if request.WaitRevision == 0 && snapshot.State.Status != durable.Idle {
		return nil, nil, errors.New("native checkpoint requires explicit same-Turn continuation")
	}
	if request.WaitRevision != 0 {
		if snapshot.Revision != request.WaitRevision {
			return nil, nil, durable.ErrConflict
		}
		if snapshot.State.Status != durable.Waiting || !settings.DurableWaits {
			return nil, nil, errors.New("committed enabled waiting checkpoint required")
		}
		for _, r := range snapshot.State.Requests {
			if !time.Now().Before(r.ExpiresAt) {
				return nil, nil, durable.ErrExpired
			}
			if len(r.Result) == 0 {
				return nil, nil, errors.New("resolve every wait before continuing")
			}
		}
	}
	if !settings.DurableWaits {
		return nil, nil, nil
	}
	target := hub.Target{EngineID: "alva", ModelID: request.Model, ProviderID: request.Provider, AccountSource: request.AccountSource, BillingRoute: request.BillingRoute}
	hash, err := hubWaitRecipe(c, request.Workspace, target, settings, compatibilitySHA)
	if err != nil {
		return nil, nil, err
	}
	if err = checkHubWaitRecipe(ctx, repo, owner, log, hash, request.WaitRevision != 0); err != nil {
		return nil, nil, err
	}
	controller, err := durable.New(durable.Config{Backend: claimedWaitBackend{backend, owned}, TTL: 24 * time.Hour})
	return backend, controller, err
}
