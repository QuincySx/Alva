// INPUT: context, database/sql, reflect, immutable blob storage contracts and native chunk metadata.
// OUTPUT: BlobConfig, storage.BlobStorageInfo, OpenWithBlobs and verified external chunk retention.
// POS: [storage adapter] Keeps metadata/facts atomic while explicit host adapters retain immutable artifact bytes.
package session

import (
	"context"
	"database/sql"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/blob"
)

// BlobConfig binds one host-owned object namespace to this repository. ID is a
// stable non-secret deployment identity, never a URL/key/credential or a model
// supplied value. The host owns the Store's lifecycle; Repository.Close does
// not close it. Volatile explicitly permits process-lifetime artifact bytes.
type BlobConfig struct {
	ID        string
	Retention blob.Retention
	Store     blob.Store
}

// OpenWithBlobs routes newly retained non-record artifacts to an explicit Store.
// Typed execution record JSON remains in the fact transaction. Existing inline
// artifacts remain readable; no bytes are moved or deleted on open. Reopening
// a bound database requires the same ID and retention, with no local fallback.
func OpenWithBlobs(ctx context.Context, directory string, config BlobConfig) (*Repository, error) {
	if !validIdentity(config.ID) || (config.Retention != blob.Persistent && config.Retention != blob.Volatile) || config.Store == nil {
		return nil, errors.New("explicit blob store, opaque binding ID and retention required")
	}
	v := reflect.ValueOf(config.Store)
	switch v.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		if v.IsNil() {
			return nil, errors.New("typed-nil blob store")
		}
	}
	return open(ctx, directory, &config)
}

// ArtifactStorage describes the configured byte retention, not the retention
// of typed execution facts. It exposes no object namespace or credentials.
func (r *Repository) ArtifactStorage() storage.BlobStorageInfo {
	if r.blobs == nil {
		return storage.BlobStorageInfo{ID: "native-sqlite", Retention: blob.Persistent}
	}
	return storage.BlobStorageInfo{ID: r.blobs.ID, Retention: r.blobs.Retention}
}

func (r *Repository) bindBlobs(ctx context.Context) error {
	var id string
	var retention blob.Retention
	err := r.db.QueryRowContext(ctx, "SELECT id,retention FROM blob_binding WHERE singleton=1").Scan(&id, &retention)
	if errors.Is(err, sql.ErrNoRows) {
		if r.blobs == nil {
			return nil
		}
		_, err = r.db.ExecContext(ctx, "INSERT INTO blob_binding(singleton,id,retention) VALUES(1,?,?)", r.blobs.ID, r.blobs.Retention)
		return err
	}
	if err != nil {
		return err
	}
	if r.blobs == nil || id != r.blobs.ID || retention != r.blobs.Retention {
		return errors.New("native repository requires its explicitly bound blob store; refusing substitution or local fallback")
	}
	return nil
}

// A full read/search amortizes remote IO over bounded windows without retaining
// a cross-request cache or skipping any chunk's catalog hash. A page read does
// not prefetch a full window beyond its normal chunk boundaries.
type blobWindow struct {
	key    string
	offset int64
	data   []byte
}

func (r *Repository) chunkData(ctx context.Context, index, total int64, data []byte, sha, key string, window *blobWindow) ([]byte, error) {
	expected := min(int64(ChunkBytes), total-index*ChunkBytes)
	if index < 0 || expected < 1 {
		return nil, errors.New("invalid artifact chunk metadata")
	}
	if key != "" {
		if r.blobs == nil || len(data) != 0 || !blob.ValidKey(key) {
			return nil, errors.New("artifact object binding is missing or corrupt")
		}
		var err error
		offset := index * ChunkBytes
		if window == nil {
			data, err = r.blobs.Store.Read(ctx, key, offset, int(expected))
		} else {
			if window.key != key || offset < window.offset || offset+expected > window.offset+int64(len(window.data)) {
				limit := int(min(int64(blob.MaxReadBytes), total-offset))
				window.data, err = r.blobs.Store.Read(ctx, key, offset, limit)
				if err != nil {
					return nil, err
				}
				if len(window.data) != limit {
					return nil, errors.New("artifact object window length mismatch")
				}
				window.key, window.offset = key, offset
			}
			start := offset - window.offset
			data = window.data[start : start+expected]
		}
		if err != nil {
			return nil, err
		}
	}
	if int64(len(data)) != expected || digest(data) != sha {
		return nil, errors.New("artifact chunk integrity verification failed")
	}
	return data, nil
}
