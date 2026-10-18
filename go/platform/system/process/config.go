// INPUT: Explicit executable, argv, workspace, environment and optional inherited file grants
// OUTPUT: Config shared across native platform process owners
// POS: [platform / OS primitive] Configuration carries no ambient credentials or product permission decisions.
package process

import "os"

type Config struct {
	Command string
	Args    []string
	Cwd     string
	Env     map[string]string
	// StdinFile grants an explicit regular file as seekable, read-only child
	// stdin instead of a pipe. Unix requires O_RDONLY; Windows duplicates only
	// GENERIC_READ/SYNCHRONIZE, never write authority. Caller retains ownership
	// and shares the file position. Process.Stdin is closed to writes and its
	// Close never closes this borrowed file. No background copy goroutine runs.
	StdinFile *os.File
	// ReadFiles are explicit regular file descriptors granted by trusted host
	// assembly, inherited as Unix fd 3+i. Windows rejects this Unix ABI rather
	// than guessing CRT descriptors or inheriting ambient handles.
	// Caller retains ownership; non-read-only descriptors are rejected.
	ReadFiles []*os.File
	// DirectoryFiles are explicit pinned directory descriptors, after ReadFiles
	// in the inherited fd sequence. Read-only open mode does NOT make the tree
	// read-only or sandbox native code; callers own cwd/escalation authority.
	DirectoryFiles []*os.File
}

type closedInput struct{}

func (closedInput) Write([]byte) (int, error) { return 0, os.ErrClosed }
func (closedInput) Close() error              { return nil }
