// INPUT: context, errors, agent/memory.
// OUTPUT: Backend.ReplaceFile atomic optional memory.FileReplacer implementation.
// POS: [platform / durable storage] File hash checkpoint, complete chunks and FTS index commit together or leave prior facts unchanged.
package memorysqlite

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/agent/memory"
)

var _ memory.FileReplacer = (*Backend)(nil)

func (b *Backend) ReplaceFile(ctx context.Context, file memory.File, chunks []memory.Chunk) error {
	if file.Path == "" || !bounded(file.Path, 4096) || !bounded(file.Source, 256) || !bounded(file.Hash, 256) || !bounded(file.MTime, 128) || file.Size < 0 || len(chunks) > 2048 {
		return errors.New("invalid atomic memory file/batch")
	}
	vectors := make([][]byte, len(chunks))
	bytes := 0
	lastLine := int64(0)
	for index, chunk := range chunks {
		if chunk.Path != file.Path || !bounded(chunk.Source, 256) || !bounded(chunk.Hash, 256) || !bounded(chunk.Text, MaxChunkBytes) || chunk.StartLine < 1 || chunk.StartLine <= lastLine || chunk.EndLine < chunk.StartLine {
			return errors.New("invalid atomic memory chunk")
		}
		bytes += len(chunk.Text)
		if bytes > 20<<20 {
			return ErrBudget
		}
		lastLine = chunk.EndLine
		data, err := encodeVector(chunk.Embedding)
		if err != nil {
			return err
		}
		vectors[index] = data
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return ErrClosed
	}
	tx, err := b.store.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err = tx.ExecContext(ctx, "DELETE FROM memory_chunks WHERE namespace=? AND path=?", b.namespace, file.Path); err != nil {
		return err
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO memory_files(namespace,path,source,hash,mtime,size) VALUES(?,?,?,?,?,?) ON CONFLICT(namespace,path) DO UPDATE SET source=excluded.source,hash=excluded.hash,mtime=excluded.mtime,size=excluded.size", b.namespace, file.Path, file.Source, file.Hash, file.MTime, file.Size); err != nil {
		return err
	}
	for index, chunk := range chunks {
		if _, err = tx.ExecContext(ctx, "INSERT INTO memory_chunks(namespace,path,source,start_line,end_line,hash,text,embedding) VALUES(?,?,?,?,?,?,?,?)", b.namespace, chunk.Path, chunk.Source, chunk.StartLine, chunk.EndLine, chunk.Hash, chunk.Text, vectors[index]); err != nil {
			return err
		}
	}
	return tx.Commit()
}
