// INPUT: Explicit desktop&&smoke QA build
// OUTPUT: Isolated temporary data directory, demo-only enforcement and own-window diagnostics
// POS: [L7 Hub product] Native QA entry; does not change production defaults, read user data or auto-start tasks
//go:build desktop && smoke

package main

import (
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/apps/debug"
	hubdesktop "github.com/QuincySx/Alva/apps/hub/desktop"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"io"
	"os"
	"path/filepath"
)

// Optional linker-owned QA parent; never consulted by production builds.
var smokeRoot string

func defaultDataDirectory() string {
	directory, err := os.MkdirTemp(smokeRoot, "alva-go-window-qa-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "native_qa: cannot create isolated data directory")
		return ""
	}
	fmt.Fprintln(os.Stderr, "native_qa: isolated data directory", directory)
	return directory
}
func validateMode(live bool) error {
	if live {
		return errors.New("smoke QA build permits local demo only, no live provider")
	}
	return nil
}
func runDesktop(service *product.HubService, diagnostics *debug.Server) error {
	mygo.App.SetName("ALVAHubSmoke")
	file, err := os.OpenFile(filepath.Join(service.DataDirectory, "native-window-qa.log"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer file.Close()
	return hubdesktop.RunDiagnosed(service, diagnostics, io.MultiWriter(os.Stderr, file))
}
