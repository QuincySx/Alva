// INPUT: context, database/sql, errors, regexp, sync, agent/memory, storage/sqlite.
// OUTPUT: Store, Backend, Open, Bind, Close and persistent memory CRUD.
// POS: [platform / durable storage] Explicit native capability backend with private namespaced SQLite, separate from session fact authority.
package memorysqlite

import (
	"context"
	"database/sql"
	"errors"
	"regexp"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/storage/sqlite"
	"github.com/QuincySx/Alva/go/sdk/agent/memory"
	"github.com/ncruces/go-sqlite3/ext/fts5"
)

var ErrClosed = errors.New("memory store is closed")
var ErrBudget = errors.New("persistent memory query exceeds explicit budget")
var namespacePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

const MaxChunkBytes = 1 << 20

type Store struct {
	mu     sync.Mutex
	db     *sql.DB
	lock   *sqlite.Lock
	closed bool
}
type Backend struct {
	store     *Store
	namespace string
}

var _ memory.Backend = (*Backend)(nil)

func Open(ctx context.Context, directory string) (*Store, error) {
	lock, err := sqlite.LockDatabase(directory, "memory.sqlite")
	if err != nil {
		return nil, err
	}
	db, err := sqlite.Open(ctx, directory, "memory.sqlite", fts5.Register)
	if err != nil {
		_ = lock.Close()
		return nil, err
	}
	store := &Store{db: db, lock: lock}
	if err = store.initialize(ctx); err != nil {
		_ = db.Close()
		_ = lock.Close()
		return nil, err
	}
	return store, nil
}
func (s *Store) initialize(ctx context.Context) error {
	var version int
	if err := s.db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return err
	}
	if version == 1 {
		return nil
	}
	if version != 0 {
		return errors.New("unsupported memory schema; no legacy import")
	}
	var tables int
	if err := s.db.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").Scan(&tables); err != nil {
		return err
	}
	if tables != 0 {
		return errors.New("unrecognized memory database; refusing implicit import")
	}
	_, err := s.db.ExecContext(ctx, `BEGIN IMMEDIATE;
 CREATE TABLE memory_files(namespace TEXT NOT NULL,path TEXT NOT NULL,source TEXT NOT NULL,hash TEXT NOT NULL,mtime TEXT NOT NULL,size INTEGER NOT NULL,PRIMARY KEY(namespace,path));
 CREATE TABLE memory_chunks(id INTEGER PRIMARY KEY AUTOINCREMENT,namespace TEXT NOT NULL,path TEXT NOT NULL,source TEXT NOT NULL,start_line INTEGER NOT NULL,end_line INTEGER NOT NULL,hash TEXT NOT NULL,text TEXT NOT NULL,embedding BLOB NOT NULL,FOREIGN KEY(namespace,path) REFERENCES memory_files(namespace,path) ON DELETE CASCADE);
 CREATE INDEX memory_chunks_path ON memory_chunks(namespace,path);
 CREATE VIRTUAL TABLE chunks_fts USING fts5(text,content='memory_chunks',content_rowid='id');
 CREATE TRIGGER chunks_fts_ai AFTER INSERT ON memory_chunks BEGIN INSERT INTO chunks_fts(rowid,text) VALUES(new.id,new.text);END;
 CREATE TRIGGER chunks_fts_ad AFTER DELETE ON memory_chunks BEGIN INSERT INTO chunks_fts(chunks_fts,rowid,text) VALUES('delete',old.id,old.text);END;
 CREATE TABLE embedding_cache(namespace TEXT NOT NULL,model TEXT NOT NULL,hash TEXT NOT NULL,embedding BLOB NOT NULL,PRIMARY KEY(namespace,model,hash));
 PRAGMA user_version=1;COMMIT;`)
	return err
}

// namespace is supplied by trusted product/workspace assembly, never accepted
// from model arguments as a grant to another workspace's memory.
func (s *Store) Bind(namespace string) (*Backend, error) {
	if !namespacePattern.MatchString(namespace) {
		return nil, errors.New("bounded opaque memory namespace required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil, ErrClosed
	}
	return &Backend{store: s, namespace: namespace}, nil
}
func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	s.closed = true
	return errors.Join(s.db.Close(), s.lock.Close())
}
func bounded(value string, limit int) bool { return len(value) <= limit && utf8.ValidString(value) }
func (b *Backend) UpsertFile(ctx context.Context, file memory.File) error {
	if file.Path == "" || !bounded(file.Path, 4096) || !bounded(file.Source, 256) || !bounded(file.Hash, 256) || !bounded(file.MTime, 128) || file.Size < 0 {
		return errors.New("invalid bounded memory file")
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return ErrClosed
	}
	_, err := b.store.db.ExecContext(ctx, "INSERT INTO memory_files(namespace,path,source,hash,mtime,size) VALUES(?,?,?,?,?,?) ON CONFLICT(namespace,path) DO UPDATE SET source=excluded.source,hash=excluded.hash,mtime=excluded.mtime,size=excluded.size", b.namespace, file.Path, file.Source, file.Hash, file.MTime, file.Size)
	return err
}
func (b *Backend) GetFile(ctx context.Context, path string) (memory.File, bool, error) {
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return memory.File{}, false, ErrClosed
	}
	var file memory.File
	err := b.store.db.QueryRowContext(ctx, "SELECT path,source,hash,mtime,size FROM memory_files WHERE namespace=? AND path=?", b.namespace, path).Scan(&file.Path, &file.Source, &file.Hash, &file.MTime, &file.Size)
	if errors.Is(err, sql.ErrNoRows) {
		return memory.File{}, false, nil
	}
	return file, err == nil, err
}
func (b *Backend) InsertChunk(ctx context.Context, chunk memory.Chunk) (int64, error) {
	embedding, err := encodeVector(chunk.Embedding)
	if err != nil {
		return 0, err
	}
	if chunk.Path == "" || !bounded(chunk.Path, 4096) || !bounded(chunk.Source, 256) || !bounded(chunk.Hash, 256) || !bounded(chunk.Text, MaxChunkBytes) || chunk.StartLine < 1 || chunk.EndLine < chunk.StartLine {
		return 0, errors.New("invalid bounded memory chunk")
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return 0, ErrClosed
	}
	result, err := b.store.db.ExecContext(ctx, "INSERT INTO memory_chunks(namespace,path,source,start_line,end_line,hash,text,embedding) VALUES(?,?,?,?,?,?,?,?)", b.namespace, chunk.Path, chunk.Source, chunk.StartLine, chunk.EndLine, chunk.Hash, chunk.Text, embedding)
	if err != nil {
		return 0, err
	}
	return result.LastInsertId()
}
func (b *Backend) DeleteChunksForPath(ctx context.Context, path string) error {
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return ErrClosed
	}
	_, err := b.store.db.ExecContext(ctx, "DELETE FROM memory_chunks WHERE namespace=? AND path=?", b.namespace, path)
	return err
}
func (b *Backend) CacheEmbedding(ctx context.Context, model, hash string, vector []float32) error {
	if model == "" || hash == "" || !bounded(model, 256) || !bounded(hash, 256) {
		return errors.New("bounded embedding model/hash required")
	}
	data, err := encodeVector(vector)
	if err != nil {
		return err
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return ErrClosed
	}
	_, err = b.store.db.ExecContext(ctx, "INSERT INTO embedding_cache(namespace,model,hash,embedding) VALUES(?,?,?,?) ON CONFLICT(namespace,model,hash) DO UPDATE SET embedding=excluded.embedding", b.namespace, model, hash, data)
	return err
}
func (b *Backend) GetCachedEmbedding(ctx context.Context, model, hash string) ([]float32, bool, error) {
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return nil, false, ErrClosed
	}
	var data []byte
	err := b.store.db.QueryRowContext(ctx, "SELECT embedding FROM embedding_cache WHERE namespace=? AND model=? AND hash=?", b.namespace, model, hash).Scan(&data)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	vector, err := decodeVector(data)
	return vector, err == nil, err
}
