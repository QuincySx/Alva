// INPUT: Explicit caller-selected credential file and native Windows private-handle validation
// OUTPUT: OpenPrivateFile returns an existing owner-only pinned read handle
// POS: [platform / OS primitive] Windows credential input is separate from Unix fd3/CRT aliases.
//go:build windows

package secretinput

import (
	"github.com/QuincySx/Alva/go/platform/system/privatefs"
	"os"
)

func OpenPrivateFile(path string) (*os.File, error) { return privatefs.OpenRead(path) }
