// INPUT: purego, macOS Security.framework SecItem* and CoreFoundation value constructors
// OUTPUT: darwin System vault of generic-password items in the user's login keychain
// POS: [platform / OS primitive] Values pass only through CFData; nothing is written to the product data directory
//go:build darwin

package keychain

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

const (
	errSecSuccess            = 0
	errSecItemNotFound       = -25300
	errSecDuplicateItem      = -25299
	kCFStringEncodingUTF8    = 0x08000100
	securityFramework        = "/System/Library/Frameworks/Security.framework/Security"
	coreFoundationFramework  = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation"
	errSecInteractionDenied  = -25308
	errSecAuthFailed         = -25293
	errSecUserCanceled       = -128
	errSecNoSuchKeychain     = -25294
	errSecNotAvailable       = -25291
	errSecInteractionDisable = -25315
)

type darwinAPI struct {
	secItemAdd          func(uintptr, uintptr) int32
	secItemCopyMatching func(uintptr, *uintptr) int32
	secItemUpdate       func(uintptr, uintptr) int32
	secItemDelete       func(uintptr) int32
	dictionaryCreate    func(uintptr, *uintptr, *uintptr, int, uintptr, uintptr) uintptr
	stringCreate        func(uintptr, *byte, int, uint32, bool) uintptr
	dataCreate          func(uintptr, *byte, int) uintptr
	dataLength          func(uintptr) int
	dataBytes           func(uintptr) *byte
	release             func(uintptr)
	keyCallbacks        uintptr
	valueCallbacks      uintptr
	booleanTrue         uintptr
	class               uintptr
	genericPassword     uintptr
	service             uintptr
	account             uintptr
	valueData           uintptr
	returnData          uintptr
	matchLimit          uintptr
	matchLimitOne       uintptr
	label               uintptr
}

var (
	apiOnce sync.Once
	api     darwinAPI
	apiErr  error
)

// global reads a CF constant: the symbol is the address of the variable.
func global(handle uintptr, name string) (uintptr, error) {
	address, err := purego.Dlsym(handle, name)
	if err != nil {
		return 0, err
	}
	return **(**uintptr)(unsafe.Pointer(&address)), nil
}

func load() (darwinAPI, error) {
	apiOnce.Do(func() {
		security, err := purego.Dlopen(securityFramework, purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			apiErr = err
			return
		}
		foundation, err := purego.Dlopen(coreFoundationFramework, purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			apiErr = err
			return
		}
		purego.RegisterLibFunc(&api.secItemAdd, security, "SecItemAdd")
		purego.RegisterLibFunc(&api.secItemCopyMatching, security, "SecItemCopyMatching")
		purego.RegisterLibFunc(&api.secItemUpdate, security, "SecItemUpdate")
		purego.RegisterLibFunc(&api.secItemDelete, security, "SecItemDelete")
		purego.RegisterLibFunc(&api.dictionaryCreate, foundation, "CFDictionaryCreate")
		purego.RegisterLibFunc(&api.stringCreate, foundation, "CFStringCreateWithBytes")
		purego.RegisterLibFunc(&api.dataCreate, foundation, "CFDataCreate")
		purego.RegisterLibFunc(&api.dataLength, foundation, "CFDataGetLength")
		purego.RegisterLibFunc(&api.dataBytes, foundation, "CFDataGetBytePtr")
		purego.RegisterLibFunc(&api.release, foundation, "CFRelease")
		if api.keyCallbacks, err = purego.Dlsym(foundation, "kCFTypeDictionaryKeyCallBacks"); err != nil {
			apiErr = err
			return
		}
		if api.valueCallbacks, err = purego.Dlsym(foundation, "kCFTypeDictionaryValueCallBacks"); err != nil {
			apiErr = err
			return
		}
		for _, entry := range []struct {
			handle uintptr
			name   string
			target *uintptr
		}{
			{foundation, "kCFBooleanTrue", &api.booleanTrue},
			{security, "kSecClass", &api.class},
			{security, "kSecClassGenericPassword", &api.genericPassword},
			{security, "kSecAttrService", &api.service},
			{security, "kSecAttrAccount", &api.account},
			{security, "kSecValueData", &api.valueData},
			{security, "kSecReturnData", &api.returnData},
			{security, "kSecMatchLimit", &api.matchLimit},
			{security, "kSecMatchLimitOne", &api.matchLimitOne},
			{security, "kSecAttrLabel", &api.label},
		} {
			if *entry.target, err = global(entry.handle, entry.name); err != nil || *entry.target == 0 {
				apiErr = errors.Join(errors.New("keychain constant unavailable: "+entry.name), err)
				return
			}
		}
	})
	return api, apiErr
}

type darwinVault struct{ service string }

func system(service string) (Vault, error) {
	if _, err := load(); err != nil {
		return nil, errors.Join(ErrUnsupported, err)
	}
	return darwinVault{service: service}, nil
}

// cf owns CoreFoundation objects created for one call.
type cf struct {
	api   darwinAPI
	owned []uintptr
}

func (c *cf) keep(ref uintptr) uintptr {
	if ref != 0 {
		c.owned = append(c.owned, ref)
	}
	return ref
}

func (c *cf) close() {
	for i := len(c.owned) - 1; i >= 0; i-- {
		c.api.release(c.owned[i])
	}
	c.owned = nil
}

func (c *cf) string(value string) uintptr {
	data := []byte(value)
	ref := c.api.stringCreate(0, &data[0], len(data), kCFStringEncodingUTF8, false)
	runtime.KeepAlive(data)
	return c.keep(ref)
}

func (c *cf) data(value []byte) uintptr {
	ref := c.api.dataCreate(0, &value[0], len(value))
	runtime.KeepAlive(value)
	return c.keep(ref)
}

func (c *cf) dictionary(pairs ...uintptr) uintptr {
	keys := make([]uintptr, 0, len(pairs)/2)
	values := make([]uintptr, 0, len(pairs)/2)
	for i := 0; i+1 < len(pairs); i += 2 {
		keys, values = append(keys, pairs[i]), append(values, pairs[i+1])
	}
	ref := c.api.dictionaryCreate(0, &keys[0], &values[0], len(keys), c.api.keyCallbacks, c.api.valueCallbacks)
	runtime.KeepAlive(keys)
	runtime.KeepAlive(values)
	return c.keep(ref)
}

func (v darwinVault) query(c *cf, account string, extra ...uintptr) uintptr {
	pairs := []uintptr{c.api.class, c.api.genericPassword, c.api.service, c.string(v.service), c.api.account, c.string(account)}
	return c.dictionary(append(pairs, extra...)...)
}

func status(code int32) error {
	switch code {
	case errSecSuccess:
		return nil
	case errSecItemNotFound:
		return ErrNotFound
	case errSecUserCanceled, errSecAuthFailed, errSecInteractionDenied, errSecInteractionDisable:
		return fmt.Errorf("keychain access was not granted (OSStatus %d)", code)
	case errSecNoSuchKeychain, errSecNotAvailable:
		return fmt.Errorf("%w (OSStatus %d)", ErrUnsupported, code)
	}
	return fmt.Errorf("keychain operation failed (OSStatus %d)", code)
}

func (v darwinVault) Get(ctx context.Context, account string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !validName(account) {
		return nil, errInvalid
	}
	c := &cf{api: api}
	defer c.close()
	var result uintptr
	code := api.secItemCopyMatching(v.query(c, account, api.returnData, api.booleanTrue, api.matchLimit, api.matchLimitOne), &result)
	if err := status(code); err != nil {
		return nil, err
	}
	c.keep(result)
	n := api.dataLength(result)
	if n <= 0 || n > MaxSecret {
		return nil, errors.New("keychain item exceeds its bound")
	}
	source := unsafe.Slice(api.dataBytes(result), n)
	return append([]byte(nil), source...), nil
}

func (v darwinVault) Put(ctx context.Context, account string, secret []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !validName(account) || !validSecret(secret) {
		return errInvalid
	}
	c := &cf{api: api}
	defer c.close()
	data := c.data(secret)
	code := api.secItemUpdate(v.query(c, account), c.dictionary(api.valueData, data))
	if code == errSecItemNotFound {
		code = api.secItemAdd(v.query(c, account, api.valueData, data, api.label, c.string(v.service+" · "+account)), 0)
	}
	return status(code)
}

func (v darwinVault) Delete(ctx context.Context, account string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !validName(account) {
		return errInvalid
	}
	c := &cf{api: api}
	defer c.close()
	if err := status(api.secItemDelete(v.query(c, account))); err != nil && !errors.Is(err, ErrNotFound) {
		return err
	}
	return nil
}

// Exists matches attributes only and never asks for the item's data.
func (v darwinVault) Exists(ctx context.Context, account string) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	if !validName(account) {
		return false, errInvalid
	}
	c := &cf{api: api}
	defer c.close()
	var result uintptr
	err := status(api.secItemCopyMatching(v.query(c, account, api.matchLimit, api.matchLimitOne), &result))
	if result != 0 {
		c.keep(result)
	}
	if errors.Is(err, ErrNotFound) {
		return false, nil
	}
	return err == nil, err
}
