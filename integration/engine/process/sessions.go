// INPUT: Private engine native binding database and confined Pi native JSONL headers
// OUTPUT: Owner/cwd exact-resume authorization and explicit adapter resource cleanup
// POS: [integration / native external-engine adapter] External native formats remain their authority; these rows only bind product ownership
package process

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func (a *Adapter) authorizeResume(ctx context.Context, request engine.Request) error {
	var owner, cwd string
	if err := a.db.QueryRowContext(ctx, `SELECT owner,cwd FROM native_bindings WHERE id=?`, request.ResumeSession).Scan(&owner, &cwd); err != nil {
		return engine.ErrSessionNotFound
	}
	if owner != request.OwnerID || cwd != filepath.Clean(request.Workspace) {
		return engine.ErrSessionNotFound
	}
	return nil
}
func (a *Adapter) piFreshSession(runID string) (string, error) {
	if !nativeID.MatchString(runID) {
		return "", errors.New("invalid new private native file identity")
	}
	directory, err := private.EnsurePrivateDir(filepath.Join(a.config.StateDirectory, "sessions"))
	if err != nil {
		return "", err
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return "", err
	}
	defer root.Close()
	file, err := root.OpenFile(runID+".jsonl", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return "", err
	}
	if err = file.Close(); err != nil {
		return "", err
	}
	return filepath.Join(directory, runID+".jsonl"), nil
}
func (a *Adapter) bindNative(ctx context.Context, request engine.Request, id string) error {
	if !nativeID.MatchString(id) {
		return errors.New("invalid native binding identity")
	}
	if request.ResumeSession != "" && id != request.ResumeSession {
		return errors.New("external engine changed the exact resumed native identity")
	}
	if _, err := a.db.ExecContext(ctx, `INSERT INTO native_bindings(id,owner,cwd) VALUES(?,?,?) ON CONFLICT(id) DO NOTHING`, id, request.OwnerID, filepath.Clean(request.Workspace)); err != nil {
		return err
	}
	copy := request
	copy.ResumeSession = id
	return a.authorizeResume(ctx, copy)
}
func (a *Adapter) piSession(request engine.Request) (string, error) {
	directory := filepath.Join(a.config.StateDirectory, "sessions")
	root, err := os.OpenRoot(directory)
	if err != nil {
		return "", engine.ErrSessionNotFound
	}
	defer root.Close()
	dir, err := root.Open(".")
	if err != nil {
		return "", err
	}
	entries, err := dir.ReadDir(2049)
	dir.Close()
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	if len(entries) > 2048 {
		return "", errors.New("private Pi session lookup exceeds 2048 entries")
	}
	found := ""
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), ".jsonl") {
			continue
		}
		info, err := root.Lstat(entry.Name())
		if err != nil {
			return "", err
		}
		if !info.Mode().IsRegular() {
			return "", errors.New("Pi native session must be a private regular non-symlink file")
		}
		if err := checkNativePrivateFile(filepath.Join(directory, entry.Name()), info); err != nil {
			return "", err
		}
		file, err := root.Open(entry.Name())
		if err != nil {
			return "", err
		}
		opened, err := file.Stat()
		if err != nil || !os.SameFile(info, opened) {
			file.Close()
			return "", errors.New("Pi native session changed during open")
		}
		reader := bufio.NewReader(io.LimitReader(file, 16<<10))
		line, err := reader.ReadString('\n')
		file.Close()
		if err != nil {
			// A fresh --session path is reserved before process startup. An
			// interrupted startup can leave it empty/incomplete; it establishes
			// no identity and must not poison unrelated exact resumes.
			continue
		}
		var header struct{ Type, ID, Cwd string }
		if json.Unmarshal([]byte(line), &header) != nil || header.Type != "session" {
			continue
		}
		if header.ID != request.ResumeSession {
			continue
		}
		if filepath.Clean(header.Cwd) != filepath.Clean(request.Workspace) {
			return "", engine.ErrSessionNotFound
		}
		if found != "" {
			return "", errors.New("duplicate exact Pi native session identity")
		}
		found = filepath.Join(directory, entry.Name())
	}
	if found == "" {
		return "", engine.ErrSessionNotFound
	}
	return found, nil
}
func (a *Adapter) Close(ctx context.Context) error {
	a.closeOnce.Do(func() {
		a.mu.Lock()
		a.closed = true
		a.stop()
		a.mu.Unlock()
		go func() { a.wg.Wait(); a.closeErr = errors.Join(a.db.Close(), a.lock.Close()); close(a.closedDone) }()
	})
	select {
	case <-a.closedDone:
		return a.closeErr
	case <-ctx.Done():
		return ctx.Err()
	}
}
