// INPUT: context, database/sql, platform-private paths, net/url and ncruces SQLite driver.
// OUTPUT: Open, EnsurePrivateDir, LockDatabase and owned Lock lifecycle.
// POS: [platform / durable storage] Shared native SQLite host plumbing with explicit private-file and process ownership.
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sync"

	"github.com/ncruces/go-sqlite3"
	"github.com/ncruces/go-sqlite3/driver"
)

var ErrLocked = errors.New("database is already owned by another process")
var ErrUnsupported = errors.New("private database ownership/locking unsupported on this platform")
var databaseName = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}\.sqlite$`)

func safePath(directory, name string) (string, error) {
	if !databaseName.MatchString(name) {
		return "", errors.New("database filename must be a bounded .sqlite basename")
	}
	canonical, err := EnsurePrivateDir(directory)
	if err != nil {
		return "", err
	}
	return filepath.Join(canonical, name), nil
}

func Open(ctx context.Context, directory, name string, initializers ...func(*sqlite3.Conn) error) (*sql.DB, error) {
	path, err := safePath(directory, name)
	if err != nil {
		return nil, err
	}
	file, err := privateFile(path)
	if err != nil {
		return nil, err
	}
	if err = file.Close(); err != nil {
		return nil, err
	}
	dsn := (&url.URL{Scheme: "file", Path: path}).String() + "?_txlock=immediate"
	db, err := driver.Open(dsn, func(connection *sqlite3.Conn) error {
		for _, pragma := range []string{"PRAGMA foreign_keys=ON", "PRAGMA journal_mode=DELETE", "PRAGMA synchronous=FULL", "PRAGMA busy_timeout=5000"} {
			if err := connection.Exec(pragma); err != nil {
				return err
			}
		}
		for _, initialize := range initializers {
			if initialize == nil {
				return errors.New("nil SQLite connection initializer")
			}
			if err := initialize(connection); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	if err = db.PingContext(ctx); err != nil {
		db.Close()
		return nil, fmt.Errorf("configure private SQLite: %w", err)
	}
	return db, nil
}

type Lock struct {
	file *os.File
	once sync.Once
	err  error
}

func LockDatabase(directory, name string) (*Lock, error) {
	path, err := safePath(directory, name)
	if err != nil {
		return nil, err
	}
	file, err := privateFile(path + ".lock")
	if err != nil {
		return nil, err
	}
	if err = lockFile(file); err != nil {
		file.Close()
		return nil, err
	}
	return &Lock{file: file}, nil
}
func (l *Lock) Close() error {
	if l == nil {
		return nil
	}
	l.once.Do(func() { l.err = errors.Join(unlockFile(l.file), l.file.Close()) })
	return l.err
}
