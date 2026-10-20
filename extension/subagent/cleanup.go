// INPUT: context, errors and sync.
// OUTPUT: Plugin.Close/CleanupComplete and retained exact child cleanup owners.
// POS: [L4 extension] Keeps unfinished child resources reachable until physical cleanup completes.
package subagent

import (
	"context"
	"errors"
	"sync"
)

type childOwner struct {
	mu    sync.Mutex
	child Child
	done  bool
}

func (p *Plugin) ownChild(child Child) *childOwner {
	if child.Close == nil {
		return nil
	}
	owner := &childOwner{child: child}
	p.mu.Lock()
	if p.children == nil {
		p.children = make(map[*childOwner]struct{})
	}
	p.children[owner] = struct{}{}
	p.mu.Unlock()
	return owner
}
func (p *Plugin) closeChild(ctx context.Context, owner *childOwner) error {
	if owner == nil {
		return nil
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.done {
		return nil
	}
	err := owner.child.Close(ctx)
	if owner.child.CleanupComplete != nil {
		owner.done = owner.child.CleanupComplete()
	} else {
		owner.done = err == nil
	}
	if owner.done {
		p.mu.Lock()
		delete(p.children, owner)
		p.mu.Unlock()
	} else if err == nil {
		err = errors.New("child physical cleanup remains incomplete")
	}
	if err != nil {
		return errors.Join(ErrChildCleanup, err)
	}
	return nil
}
func (p *Plugin) closeChildren(ctx context.Context) error {
	p.mu.Lock()
	owners := make([]*childOwner, 0, len(p.children))
	for owner := range p.children {
		owners = append(owners, owner)
	}
	p.mu.Unlock()
	var err error
	for _, owner := range owners {
		err = errors.Join(err, p.closeChild(ctx, owner))
	}
	return err
}

// Close is terminal. The execution owner must cancel and join Run first;
// unfinished resources remain owned and Close can be called again.
func (p *Plugin) Close(ctx context.Context) error {
	p.mu.Lock()
	p.closed = true
	active := p.runID != ""
	p.mu.Unlock()
	err := p.closeChildren(ctx)
	if active {
		err = errors.Join(err, ErrChildCleanup, errors.New("child execution owner is still active"))
	}
	return err
}
func (p *Plugin) CleanupComplete() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.runID == "" && len(p.children) == 0
}
