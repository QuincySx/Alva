// INPUT: context, crypto/rand, encoding/hex and errors.
// OUTPUT: Store, bounded immutable object contracts, retention declarations and opaque keys.
// POS: [storage port] Separates byte retention from session authorization, fact transactions and filesystem APIs.
package blob

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
)

const MaxObjectBytes = 32 << 20
const MaxReadBytes = 64 << 10

var ErrNotFound = errors.New("blob is unavailable")
var ErrConflict = errors.New("immutable blob key already contains different bytes")
var ErrLimit = errors.New("blob storage budget exceeded")
var ErrInvalid = errors.New("invalid bounded blob key, data or range")

// Store is a host-owned, already authorized object namespace. Keys are random
// opaque identities, never paths, credentials or authorization for model calls.
// Put publishes a complete immutable value: an identical retry is idempotent,
// different bytes return ErrConflict. Success requires read-after-write
// visibility. Errors, including cancellation, may follow publication; callers
// must not infer rollback, automatically retry, or delete an uncertain write.
// Put must not mutate/retain the input buffer; Read returns an owned buffer of
// exactly min(limit, size-offset) bytes. Reads past size fail (at size is empty).
// Implementations must support concurrent calls, including competing creators.
// The adapter documents its physical retention. No distributed transaction,
// execution claim, listing, deletion or credential discovery is implied.
type Store interface {
	Put(context.Context, string, []byte) error
	Read(context.Context, string, int64, int) ([]byte, error)
}

type Retention string

const Persistent Retention = "persistent"
const Volatile Retention = "process-lifetime"

func NewKey() (string, error) {
	var data [32]byte
	if _, err := rand.Read(data[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(data[:]), nil
}

func ValidKey(key string) bool {
	if len(key) != 64 {
		return false
	}
	for _, c := range key {
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

func ValidatePut(key string, data []byte) error {
	if !ValidKey(key) || len(data) > MaxObjectBytes {
		return ErrInvalid
	}
	return nil
}

func ValidateRead(key string, offset int64, limit int) error {
	if !ValidKey(key) || offset < 0 || offset > MaxObjectBytes || limit < 1 || limit > MaxReadBytes {
		return ErrInvalid
	}
	return nil
}
