// INPUT: Normal desktop build, explicit CLI flags or a no-argument installed bundle launch profile
// OUTPUT: Required explicit data directory or installed profile, plus ordinary product window runner
// POS: [L7 Hub product] Keeps production startup defaults independent from temporary smoke fixtures
//go:build desktop && !smoke

package main

import (
	"context"
	"io"

	"github.com/QuincySx/Alva/apps/debug"
	hubdesktop "github.com/QuincySx/Alva/apps/hub/desktop"
	"github.com/QuincySx/Alva/apps/hub/product"
)

func prepareDesktopFromExecutable(ctx context.Context, executable string, args []string, input io.Reader, diagnostic io.Writer) (*desktopStartup, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	launch, err := loadDesktopLaunchProfile(executable, args)
	if err != nil {
		return nil, err
	}
	return prepareDesktopProfile(ctx, launch, input, diagnostic)
}

func defaultDataDirectory() string { return "" }
func validateMode(bool) error      { return nil }
func runDesktop(service *product.HubService, diagnostics *debug.Server) error {
	return hubdesktop.RunDiagnosed(service, diagnostics, nil)
}
