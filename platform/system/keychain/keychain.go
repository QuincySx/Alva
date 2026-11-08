// INPUT: context, explicit service/account names and small secret byte values
// OUTPUT: Vault, System, Memory, ErrNotFound, ErrUnsupported and MaxSecret
// POS: [platform / OS primitive] Secret storage in the OS keychain only; never a plaintext file fallback
package keychain

import (
	"context"
	"errors"
	"sync"
)

// MaxSecret bounds one stored value.
const MaxSecret = 16 << 10

var (
	ErrNotFound    = errors.New("keychain item not found")
	ErrUnsupported = errors.New("system keychain is unavailable on this platform")
	errInvalid     = errors.New("keychain requires a bounded printable account and secret")
)

// Vault stores small secrets under one service. Exists never reads secret
// data, so listing status does not ask the user to unlock individual items.
type Vault interface {
	Get(context.Context, string) ([]byte, error)
	Put(context.Context, string, []byte) error
	Delete(context.Context, string) error
	Exists(context.Context, string) (bool, error)
}

func validName(value string) bool {
	if value == "" || len(value) > 256 {
		return false
	}
	for _, r := range value {
		if r < 0x21 || r > 0x7e {
			return false
		}
	}
	return true
}

func validSecret(secret []byte) bool { return len(secret) > 0 && len(secret) <= MaxSecret }

// System returns the operating-system keychain for service. Platforms without
// an implementation return ErrUnsupported; callers must not fall back to disk.
func System(service string) (Vault, error) {
	if !validName(service) {
		return nil, errInvalid
	}
	return system(service)
}

// Memory is an in-process vault for tests and for programs that explicitly
// keep nothing after exit.
type Memory struct {
	mu    sync.Mutex
	items map[string][]byte
}

func NewMemory() *Memory { return &Memory{items: map[string][]byte{}} }

func (m *Memory) Get(ctx context.Context, account string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	value, ok := m.items[account]
	if !ok {
		return nil, ErrNotFound
	}
	return append([]byte(nil), value...), nil
}

func (m *Memory) Put(ctx context.Context, account string, secret []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !validName(account) || !validSecret(secret) {
		return errInvalid
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.items[account] = append([]byte(nil), secret...)
	return nil
}

func (m *Memory) Delete(ctx context.Context, account string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.items, account)
	return nil
}

func (m *Memory) Exists(ctx context.Context, account string) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	_, ok := m.items[account]
	return ok, nil
}
