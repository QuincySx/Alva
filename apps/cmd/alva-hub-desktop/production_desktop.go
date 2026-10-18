// INPUT: Normal desktop build without QA smoke tag
// OUTPUT: Required explicit data directory and ordinary product window runner
// POS: [L7 Hub product] Keeps production startup defaults independent from temporary smoke fixtures
//go:build desktop && !smoke

package main

import (
	"github.com/QuincySx/Alva/apps/debug"
	hubdesktop "github.com/QuincySx/Alva/apps/hub/desktop"
	"github.com/QuincySx/Alva/apps/hub/product"
)

func defaultDataDirectory() string { return "" }
func validateMode(bool) error      { return nil }
func runDesktop(service *product.HubService, diagnostics *debug.Server) error {
	return hubdesktop.RunDiagnosed(service, diagnostics, nil)
}
