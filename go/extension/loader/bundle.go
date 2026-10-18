// INPUT: Explicit source directory/expected manifest and private snapshot parent
// OUTPUT: ReadManifest, Stage, Bundle root/entry/digest and exact owned cleanup
// POS: [L4 extension] Bounded no-symlink snapshot before external Plugin execution; never runs mutable discovery files
package loader

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"hash"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"sync"
	"unicode/utf8"
)

type Bundle struct {
	directory, entry, digest string
	parent, root             *os.Root
	name                     string
	once                     sync.Once
	err                      error
	manifest                 Manifest
}

func (b *Bundle) Root() string   { return b.directory }
func (b *Bundle) Entry() string  { return b.entry }
func (b *Bundle) Digest() string { return b.digest }

// ValidateForLaunch rejects path identity or code changes after staging. The
// process owner must call it immediately before launch and keep Bundle alive
// until the owned child is reaped. This is not an OS sandbox against same-UID
// native code; private approved ancestor directories remain a host requirement.
func (b *Bundle) ValidateForLaunch(ctx context.Context) error {
	if b.root == nil || b.directory == "" {
		return errors.New("invalid Plugin snapshot")
	}
	opened, err := b.root.Stat(".")
	if err != nil {
		return err
	}
	actual, err := os.Lstat(b.directory)
	if err != nil || !actual.IsDir() || !os.SameFile(opened, actual) {
		return errors.New("Plugin snapshot execution path no longer matches pinned directory")
	}
	var files []string
	budget := copyBudget{}
	if err = copyTree(ctx, b.root, nil, "", 0, &budget, &files); err != nil {
		return err
	}
	data, err := readRegular(ctx, b.root, "alva.toml", 64<<10)
	if err != nil {
		return err
	}
	manifest, err := ParseManifest(data)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(manifest, b.manifest) {
		return errors.New("Plugin snapshot manifest changed before launch")
	}
	digest, err := digestSnapshot(ctx, b.root, files, manifest)
	if err != nil {
		return err
	}
	if digest != b.digest {
		return errors.New("Plugin snapshot content changed before launch")
	}
	return nil
}

func openSource(path string) (*os.Root, error) {
	if !filepath.IsAbs(path) {
		return nil, errors.New("explicit absolute Plugin directory required")
	}
	before, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("Plugin directory must not be a symlink")
	}
	root, err := os.OpenRoot(path)
	if err != nil {
		return nil, err
	}
	actual, err := root.Stat(".")
	if err != nil || !os.SameFile(before, actual) {
		root.Close()
		return nil, errors.New("Plugin source changed during open")
	}
	return root, nil
}
func readRegular(ctx context.Context, root *os.Root, name string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	before, err := root.Lstat(name)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || before.Size() > limit {
		return nil, errors.New("Plugin requires bounded regular files without symlinks")
	}
	f, err := root.Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("Plugin file changed during open")
	}
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return nil, err
	}
	after, err := f.Stat()
	if err != nil || int64(len(data)) > limit || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
		return nil, errors.New("Plugin file changed or exceeded budget")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}
func ReadManifest(ctx context.Context, source string) (Manifest, error) {
	root, err := openSource(source)
	if err != nil {
		return Manifest{}, err
	}
	defer root.Close()
	data, err := readRegular(ctx, root, "alva.toml", 64<<10)
	if err != nil {
		return Manifest{}, err
	}
	return ParseManifest(data)
}

// Stage never discovers sources or starts a process. snapshotParent must be an
// explicit owner-only real directory. Digest uses the new Go v1 canonical
// manifest format, not an implicit converter for historical Rust pins/data.
func Stage(ctx context.Context, source, snapshotParent string, expected Manifest) (result *Bundle, err error) {
	if err = expected.Validate(); err != nil {
		return nil, err
	}
	root, err := openSource(source)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	parentBefore, err := os.Lstat(snapshotParent)
	if err != nil {
		return nil, err
	}
	if !parentBefore.IsDir() || parentBefore.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("Plugin snapshot parent must be an explicit real directory")
	}
	snapshotParent, err = filepath.EvalSymlinks(snapshotParent)
	if err != nil {
		return nil, err
	}
	parent, err := openSource(snapshotParent)
	if err != nil {
		return nil, err
	}
	info, err := parent.Stat(".")
	if err != nil {
		parent.Close()
		return nil, err
	}
	if !os.SameFile(parentBefore, info) {
		parent.Close()
		return nil, errors.New("Plugin snapshot parent changed while pinning")
	}
	if err = checkSnapshotOwner(snapshotParent, info); err != nil {
		parent.Close()
		return nil, err
	}
	var random [16]byte
	if _, err = rand.Read(random[:]); err != nil {
		parent.Close()
		return nil, err
	}
	name := "plugin-" + hex.EncodeToString(random[:])
	if err = parent.Mkdir(name, 0700); err != nil {
		parent.Close()
		return nil, err
	}
	manifestCopy, _ := json.Marshal(expected)
	var immutableManifest Manifest
	_ = json.Unmarshal(manifestCopy, &immutableManifest)
	bundle := &Bundle{directory: filepath.Join(snapshotParent, name), entry: filepath.Join(snapshotParent, name, filepath.FromSlash(expected.Entry)), parent: parent, name: name, manifest: immutableManifest}
	defer func() {
		if err != nil {
			err = errors.Join(err, bundle.Close())
		}
	}()
	bundle.root, err = parent.OpenRoot(name)
	if err != nil {
		return nil, err
	}
	budget := copyBudget{}
	var files []string
	if err = copyTree(ctx, root, bundle.root, "", 0, &budget, &files); err != nil {
		return nil, err
	}
	data, err := readRegular(ctx, bundle.root, "alva.toml", 64<<10)
	if err != nil {
		return nil, err
	}
	manifest, err := ParseManifest(data)
	if err != nil {
		return nil, err
	}
	if !reflect.DeepEqual(expected, manifest) {
		return nil, errors.New("Plugin manifest changed before snapshot approval")
	}
	if _, err = readRegular(ctx, bundle.root, expected.Entry, 8<<20); err != nil {
		return nil, err
	}
	bundle.digest, err = digestSnapshot(ctx, bundle.root, files, manifest)
	if err != nil {
		return nil, err
	}
	if expected.ContentSHA256 != nil && !strings.EqualFold(*expected.ContentSHA256, bundle.digest) {
		return nil, errors.New("Plugin snapshot digest pin mismatch")
	}
	return bundle, nil
}
func digestSnapshot(ctx context.Context, root *os.Root, files []string, manifest Manifest) (string, error) {
	sort.Strings(files)
	hasher := sha256.New()
	hasher.Write([]byte("alva.bundle.go.v1\x00"))
	for _, file := range files {
		var data []byte
		var err error
		frameHash(hasher, []byte(file))
		if file == "alva.toml" {
			manifest.ContentSHA256 = nil
			if manifest.RequestedCapabilities == nil {
				manifest.RequestedCapabilities = []string{}
			}
			data, err = json.Marshal(manifest)
		} else {
			data, err = readRegular(ctx, root, file, 8<<20)
		}
		if err != nil {
			return "", err
		}
		frameHash(hasher, data)
	}
	return hex.EncodeToString(hasher.Sum(nil)), nil
}
func frameHash(h hash.Hash, data []byte) {
	var length [8]byte
	binary.BigEndian.PutUint64(length[:], uint64(len(data)))
	h.Write(length[:])
	h.Write(data)
}

type copyBudget struct{ entries, files, bytes int }

func copyTree(ctx context.Context, source, target *os.Root, relative string, depth int, budget *copyBudget, files *[]string) error {
	if depth > 16 {
		return errors.New("Plugin bundle depth exceeds 16")
	}
	f, err := source.Open(".")
	if err != nil {
		return err
	}
	entries, readErr := f.ReadDir(513 - budget.entries)
	closeErr := f.Close()
	if readErr != nil && readErr != io.EOF {
		return readErr
	}
	if closeErr != nil {
		return closeErr
	}
	budget.entries += len(entries)
	if budget.entries > 512 {
		return errors.New("Plugin bundle exceeds 512 entries")
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name() < entries[j].Name() })
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return err
		}
		name := entry.Name()
		if !utf8.ValidString(name) || strings.ContainsAny(name, "/\\") || name == "." || name == ".." {
			return errors.New("invalid Plugin file name")
		}
		before, err := source.Lstat(name)
		if err != nil {
			return err
		}
		if before.Mode()&os.ModeSymlink != 0 {
			return errors.New("Plugin bundle symlinks rejected")
		}
		path := name
		if relative != "" {
			path = relative + "/" + name
		}
		if before.IsDir() {
			child, err := source.OpenRoot(name)
			if err != nil {
				return err
			}
			actual, err := child.Stat(".")
			if err != nil || !os.SameFile(before, actual) {
				child.Close()
				return errors.New("Plugin source directory changed")
			}
			if target != nil {
				err = target.Mkdir(path, 0700)
			}
			if err == nil {
				err = copyTree(ctx, child, target, path, depth+1, budget, files)
			}
			child.Close()
			if err != nil {
				return err
			}
			continue
		}
		if !before.Mode().IsRegular() {
			return errors.New("Plugin special files rejected")
		}
		budget.files++
		if budget.files > 256 {
			return errors.New("Plugin bundle exceeds 256 files")
		}
		data, err := readRegular(ctx, source, name, 8<<20)
		if err != nil {
			return err
		}
		budget.bytes += len(data)
		if budget.bytes > 16<<20 {
			return errors.New("Plugin bundle exceeds 16 MiB")
		}
		if target != nil {
			out, err := target.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
			if err != nil {
				return err
			}
			n, writeErr := out.Write(data)
			if writeErr == nil && n != len(data) {
				writeErr = io.ErrShortWrite
			}
			err = errors.Join(writeErr, out.Sync(), out.Close())
			if err != nil {
				return err
			}
		}
		*files = append(*files, path)
	}
	return nil
}
func (b *Bundle) Close() error {
	b.once.Do(func() {
		if b.root != nil {
			b.err = b.root.Close()
		}
		if b.parent != nil {
			b.err = errors.Join(b.err, b.parent.RemoveAll(b.name), b.parent.Close())
		}
	})
	return b.err
}
