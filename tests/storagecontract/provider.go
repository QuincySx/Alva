// INPUT: Shared storage/SDK contracts only, with a host-supplied provider.
// OUTPUT: Wrap and opaque session/reference decorators with call observations.
// POS: [test support] Proves consumers use the selected contract end to end without recovering native implementation handles.
package storagecontract

import (
	"context"
	"encoding/json"
	"sync"

	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Provider struct {
	storage.Repository
	mu    sync.Mutex
	calls map[string]int
}

func Wrap(provider storage.Repository) *Provider {
	return &Provider{Repository: provider, calls: make(map[string]int)}
}
func (p *Provider) note(name string) {
	p.mu.Lock()
	p.calls[name]++
	p.mu.Unlock()
}
func (p *Provider) Calls(name string) int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.calls[name]
}
func (p *Provider) session(s storage.Session, err error) (storage.Session, error) {
	if err != nil || storage.Missing(s) {
		return nil, err
	}
	return &session{Session: s, provider: p}, nil
}
func (p *Provider) Create(ctx context.Context, owner, id, parent string) (storage.Session, error) {
	p.note("create")
	return p.session(p.Repository.Create(ctx, owner, id, parent))
}
func (p *Provider) OpenSession(ctx context.Context, owner, id string) (storage.Session, error) {
	p.note("open")
	return p.session(p.Repository.OpenSession(ctx, owner, id))
}
func (p *Provider) CreateChild(ctx context.Context, owner, parent, id, run, call, scope string) (storage.ChildCreation, error) {
	p.note("child")
	created, err := p.Repository.CreateChild(ctx, owner, parent, id, run, call, scope)
	if err != nil {
		return created, err
	}
	created.Session, err = p.session(created.Session, nil)
	return created, err
}
func (p *Provider) BoundReferences(ctx context.Context, owner, id string) (agentcontext.ReferenceBackend, error) {
	p.note("references")
	backend, err := p.Repository.BoundReferences(ctx, owner, id)
	if err != nil {
		return nil, err
	}
	return &references{backend: backend, provider: p}, nil
}
func (p *Provider) PutArtifact(ctx context.Context, owner, id string, input storage.ArtifactInput) (storage.ArtifactInfo, error) {
	p.note("artifact")
	return p.Repository.PutArtifact(ctx, owner, id, input)
}
func (p *Provider) Close() error { p.note("close"); return p.Repository.Close() }

type session struct {
	storage.Session
	provider *Provider
}

func (s *session) SameAuthority(other abi.ConversationLog) bool {
	actual, ok := other.(*session)
	return ok && actual != nil && actual.provider == s.provider && s.Session.SameAuthority(actual.Session)
}
func (s *session) AppendRecords(ctx context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	s.provider.note("append")
	return s.Session.AppendRecords(ctx, records)
}
func (s *session) AppendExtensionSnapshot(ctx context.Context, namespace, kind string, expected uint64, payload json.RawMessage, additional ...abi.SessionRecord) (abi.SessionRecord, error) {
	s.provider.note("snapshot")
	return s.Session.AppendExtensionSnapshot(ctx, namespace, kind, expected, payload, additional...)
}

type references struct {
	backend  agentcontext.ReferenceBackend
	provider *Provider
}

func (r *references) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	r.provider.note("reference_store")
	return r.backend.StoreText(ctx, text)
}
func (r *references) ReadText(ctx context.Context, uri string, offset int64, limit int) (agentcontext.TextPage, error) {
	r.provider.note("reference_read")
	return r.backend.ReadText(ctx, uri, offset, limit)
}
func (r *references) SearchText(ctx context.Context, uri, query string, limit int) ([]agentcontext.TextMatch, error) {
	r.provider.note("reference_search")
	return r.backend.SearchText(ctx, uri, query, limit)
}

var _ storage.Repository = (*Provider)(nil)
var _ storage.Session = (*session)(nil)
