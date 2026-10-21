// INPUT: context, os.Root, io/fs, MIME and Skill protocol values
// OUTPUT: FS, Root, Open, OpenWithin, OpenAgentSkillsWithin, OpenPinned, Close, List, Load, Resource, Resources, Install, Archive, Restore
// POS: [integration / external protocol] Explicit bounded skill roots with pinned descriptors and recoverable managed writes
package skill

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"mime"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"sync"
)

type Root struct {
	Path     string
	Writable bool
}
type pinned struct {
	root     *os.Root
	writable bool
}
type FS struct {
	mu       sync.Mutex
	roots    []pinned
	index    map[string]located
	standard bool
}

// OpenAgentSkillsWithin explicitly selects the Agent Skills document format.
// This read-only repository retains the same confined filesystem boundaries;
// ordinary Open/OpenWithin and Parse continue to use Alva's native format.
func OpenAgentSkillsWithin(workspace, relative string) (*FS, error) {
	r, err := OpenWithin(workspace, relative, false)
	if err == nil {
		r.standard = true
	}
	return r, err
}

// OpenPinned duplicates an explicit directory capability into a read-only
// repository. The caller retains its descriptor; no textual path is reopened.
func OpenPinned(root *os.Root, agentSkills bool) (*FS, error) {
	if root == nil {
		return nil, errors.New("explicit pinned skill root required")
	}
	child, err := root.OpenRoot(".")
	if err != nil {
		return nil, err
	}
	return &FS{roots: []pinned{{root: child}}, standard: agentSkills}, nil
}

func (r *FS) parse(data []byte) (Document, error) {
	if r.standard {
		return ParseAgentSkills(data)
	}
	return Parse(data)
}

// OpenWithin pins an existing project Skill directory without resolving its
// relative ancestors outside the approved workspace. FS owns only its new
// child descriptor; no caller descriptor is consumed and no directory created.
func OpenWithin(workspace, relative string, writable bool) (*FS, error) {
	if !filepath.IsAbs(workspace) || relative == "." || !fs.ValidPath(relative) || strings.Contains(relative, "\\") {
		return nil, errors.New("absolute workspace and confined relative skill directory required")
	}
	parent, err := os.OpenRoot(workspace)
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	var before os.FileInfo
	parts := strings.Split(relative, "/")
	for i := range parts {
		before, err = parent.Lstat(strings.Join(parts[:i+1], "/"))
		if err != nil {
			return nil, err
		}
		if !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("project skill directory ancestors must be real directories")
		}
	}
	child, err := parent.OpenRoot(relative)
	if err != nil {
		return nil, err
	}
	actual, err := child.Stat(".")
	if err != nil || !os.SameFile(before, actual) {
		child.Close()
		return nil, errors.New("project skill directory changed during open")
	}
	return &FS{roots: []pinned{{root: child, writable: writable}}}, nil
}

func Open(roots []Root) (*FS, error) {
	if len(roots) == 0 || len(roots) > 16 {
		return nil, errors.New("1 to 16 explicit skill roots required")
	}
	repo := &FS{}
	writes := 0
	for _, config := range roots {
		if !filepath.IsAbs(config.Path) {
			repo.Close()
			return nil, errors.New("skill roots must be absolute")
		}
		info, err := os.Lstat(config.Path)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			repo.Close()
			return nil, errors.New("skill root must be an existing real directory")
		}
		r, err := os.OpenRoot(config.Path)
		if err != nil {
			repo.Close()
			return nil, err
		}
		repo.roots = append(repo.roots, pinned{r, config.Writable})
		if config.Writable {
			writes++
		}
	}
	if writes > 1 {
		repo.Close()
		return nil, errors.New("only one managed writable root may be configured")
	}
	return repo, nil
}
func (r *FS) Close() error {
	var err error
	for _, p := range r.roots {
		err = errors.Join(err, p.root.Close())
	}
	return err
}
func read(ctx context.Context, r *os.Root, name string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !fs.ValidPath(name) || strings.Contains(name, "\\") {
		return nil, errors.New("invalid skill relative path")
	}
	parts := strings.Split(name, "/")
	for i := range parts {
		st, err := r.Lstat(strings.Join(parts[:i+1], "/"))
		if err != nil {
			return nil, err
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("skill symlinks are not allowed")
		}
	}
	before, err := r.Lstat(name)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || before.Size() > limit {
		return nil, errors.New("skill resource is not a bounded regular file")
	}
	f, err := r.Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("skill resource changed during open")
	}
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limit {
		return nil, errors.New("skill resource exceeds byte limit")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}

type located struct {
	skill     Skill
	root      *os.Root
	directory string
}

func (r *FS) catalog(ctx context.Context) (map[string]located, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if r.index != nil {
		return r.index, nil
	}
	index := make(map[string]located)
	readBytes := 0
	for _, p := range r.roots {
		d, err := p.root.Open(".")
		if err != nil {
			return nil, err
		}
		entries, err := d.ReadDir(2049)
		d.Close()
		if err != nil && !errors.Is(err, io.EOF) {
			return nil, err
		}
		if len(entries) > 2048 {
			return nil, errors.New("skill root entry limit exceeded")
		}
		for _, e := range entries {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			if strings.HasPrefix(e.Name(), ".") {
				continue
			}
			if e.Type()&os.ModeSymlink != 0 {
				return nil, errors.New("skill root contains a symlink")
			}
			if !e.IsDir() {
				continue
			}
			_, err := p.root.Lstat(e.Name() + "/SKILL.md")
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return nil, err
			}
			data, err := read(ctx, p.root, e.Name()+"/SKILL.md", MaxDocumentBytes)
			if err != nil {
				return nil, err
			}
			readBytes += len(data)
			if readBytes > 32<<20 {
				return nil, errors.New("skill discovery exceeds 32 MiB")
			}
			doc, err := r.parse(data)
			if err != nil {
				return nil, fmt.Errorf("skill %q: %w", e.Name(), err)
			}
			if doc.Meta.Name != e.Name() {
				return nil, errors.New("skill directory and manifest name differ")
			}
			index[doc.Meta.Name] = located{Skill{Meta: doc.Meta, Source: filepath.Join(p.root.Name(), e.Name()), Enabled: true}, p.root, e.Name()}
			if len(index) > 1024 {
				return nil, errors.New("skill count exceeds 1024")
			}
		}
	}
	r.index = index
	return index, nil
}

// Refresh is explicit: external edits do not trigger repeated filesystem scans in every model turn.
func (r *FS) Refresh(ctx context.Context) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.index = nil
	_, err := r.catalog(ctx)
	return err
}
func (r *FS) List(ctx context.Context) ([]Skill, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	index, err := r.catalog(ctx)
	if err != nil {
		return nil, err
	}
	out := make([]Skill, 0, len(index))
	for _, s := range index {
		detached := s.skill
		data, _ := json.Marshal(detached.Meta)
		detached.Meta = Meta{}
		if err := json.Unmarshal(data, &detached.Meta); err != nil {
			return nil, err
		}
		out = append(out, detached)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Meta.Name < out[j].Meta.Name })
	return out, nil
}
func (r *FS) find(ctx context.Context, name string) (located, error) {
	if !ValidName(name) {
		return located{}, errors.New("invalid skill name")
	}
	index, err := r.catalog(ctx)
	if err != nil {
		return located{}, err
	}
	s, ok := index[name]
	if !ok {
		return located{}, os.ErrNotExist
	}
	return s, nil
}
func (r *FS) Load(ctx context.Context, name string) (Document, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	s, err := r.find(ctx, name)
	if err != nil {
		return Document{}, err
	}
	data, err := read(ctx, s.root, s.directory+"/SKILL.md", MaxDocumentBytes)
	if err != nil {
		return Document{}, err
	}
	doc, err := r.parse(data)
	if err != nil {
		return Document{}, err
	}
	if doc.Meta.Name != name {
		return Document{}, errors.New("skill identity changed; refresh required")
	}
	return doc, nil
}
func (r *FS) Resource(ctx context.Context, name, relative string) (Resource, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if !fs.ValidPath(relative) || strings.Contains(relative, "\\") {
		return Resource{}, errors.New("invalid skill resource path")
	}
	s, err := r.find(ctx, name)
	if err != nil {
		return Resource{}, err
	}
	data, err := read(ctx, s.root, s.directory+"/"+relative, MaxDocumentBytes)
	if err != nil {
		return Resource{}, err
	}
	kind := mime.TypeByExtension(path.Ext(relative))
	if kind == "" {
		kind = "application/octet-stream"
	}
	return Resource{Path: relative, MediaType: kind, Bytes: data}, nil
}
func (r *FS) Resources(ctx context.Context, name string) ([]string, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	s, err := r.find(ctx, name)
	if err != nil {
		return nil, err
	}
	var out []string
	nodes := 0
	err = fs.WalkDir(s.root.FS(), s.directory, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		nodes++
		if nodes > 2048 || strings.Count(p, "/") > 16 {
			return errors.New("skill resource tree exceeds limits")
		}
		if d.Type()&os.ModeSymlink != 0 {
			return errors.New("skill symlinks are not allowed")
		}
		if !d.IsDir() && p != s.directory+"/SKILL.md" {
			info, err := d.Info()
			if err != nil {
				return err
			}
			if !info.Mode().IsRegular() {
				return errors.New("skill contains a special file")
			}
			out = append(out, strings.TrimPrefix(p, s.directory+"/"))
		}
		return nil
	})
	sort.Strings(out)
	return out, err
}
func (r *FS) managed() (*os.Root, error) {
	for _, p := range r.roots {
		if p.writable {
			return p.root, nil
		}
	}
	return nil, errors.New("no managed writable skill root")
}
func randomID() (string, error) {
	var b [12]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}
func (r *FS) Install(ctx context.Context, name string, files map[string][]byte) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if !ValidName(name) || len(files) == 0 || len(files) > 128 {
		return errors.New("invalid skill bundle")
	}
	doc, err := r.parse(files["SKILL.md"])
	if err != nil {
		return err
	}
	if doc.Meta.Name != name {
		return errors.New("skill identity mismatch")
	}
	total := 0
	for p, data := range files {
		if !fs.ValidPath(p) || strings.Contains(p, "\\") || strings.Count(p, "/") > 16 || len(data) > MaxDocumentBytes {
			return errors.New("invalid bundle path or resource size")
		}
		total += len(data)
	}
	if total > 20<<20 {
		return errors.New("skill bundle exceeds 20 MiB")
	}
	root, err := r.managed()
	if err != nil {
		return err
	}
	if _, err = root.Lstat(name); err == nil {
		return errors.New("skill already exists; archive it explicitly before replacement")
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	id, err := randomID()
	if err != nil {
		return err
	}
	stage := ".install-" + id
	if err = root.Mkdir(stage, 0700); err != nil {
		return err
	}
	// Only the exact freshly-created staging directory is removed on failed installation.
	defer root.RemoveAll(stage)
	for p, data := range files {
		if err := ctx.Err(); err != nil {
			return err
		}
		target := stage + "/" + p
		if parent := path.Dir(target); parent != "." {
			if err = root.MkdirAll(parent, 0700); err != nil {
				return err
			}
		}
		f, err := root.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			return err
		}
		_, writeErr := f.Write(data)
		syncErr := f.Sync()
		closeErr := f.Close()
		if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
			return err
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	err = root.Rename(stage, name)
	if err == nil {
		r.index = nil
	}
	return err
}
func (r *FS) Archive(ctx context.Context, name string) (string, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if !ValidName(name) {
		return "", errors.New("invalid skill name")
	}
	root, err := r.managed()
	if err != nil {
		return "", err
	}
	if _, err = read(ctx, root, name+"/SKILL.md", MaxDocumentBytes); err != nil {
		return "", err
	}
	if err = root.MkdirAll(".archive", 0700); err != nil {
		return "", err
	}
	st, err := root.Lstat(".archive")
	if err != nil || !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
		return "", errors.New("invalid archive directory")
	}
	id, err := randomID()
	if err != nil {
		return "", err
	}
	archive := name + "-" + id
	if err = root.Rename(name, ".archive/"+archive); err != nil {
		return "", err
	}
	r.index = nil
	return archive, nil
}
func (r *FS) Restore(ctx context.Context, archive string) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if !fs.ValidPath(archive) || strings.ContainsAny(archive, "/\\") {
		return errors.New("invalid archive identity")
	}
	root, err := r.managed()
	if err != nil {
		return err
	}
	data, err := read(ctx, root, ".archive/"+archive+"/SKILL.md", MaxDocumentBytes)
	if err != nil {
		return err
	}
	doc, err := r.parse(data)
	if err != nil {
		return err
	}
	if _, err = root.Lstat(doc.Meta.Name); err == nil {
		return errors.New("restore target already exists")
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	err = root.Rename(".archive/"+archive, doc.Meta.Name)
	if err == nil {
		r.index = nil
	}
	return err
}
