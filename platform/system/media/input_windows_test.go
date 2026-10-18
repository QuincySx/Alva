// INPUT: Windows protected DACL root, native delete-on-close input and actual owned helper executable
// OUTPUT: Native Windows snapshot lifecycle and handle-only decoder binding acceptance
// POS: [platform / OS primitive] Compile checks are separate from running these actual Windows handle tests.
//go:build windows

package media

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/QuincySx/Alva/platform/system/process"
)

func TestWindowsInputSnapshotAndHandleBinding(t *testing.T) {
	directory, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	exerciseInputSnapshot(t, directory)
	read, err := inputSnapshot(directory, "binding-", []byte("tail"))
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	var config process.Config
	bindDecoderInput(&config, read)
	args, err := decoderInput("video/mp4")
	if err != nil || config.StdinFile != read || len(config.ReadFiles) != 0 || args[len(args)-1] != "handle:" || strings.Contains(strings.Join(args, " "), "/dev/fd/") || decoderProtocols != "handle,pipe" {
		t.Fatal("Windows decoder kept Unix fd or host-path fallback", args, err)
	}
}
