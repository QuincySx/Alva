// INPUT: Installed bundle executable, bounded explicit Resources launch manifest and existing bootstrap external snapshots
// OUTPUT: No-argument bundle launch options and approved per-engine PATH without ambient configuration discovery
// POS: [L7 Hub product] Reuses normal desktop startup for an explicitly installed product profile; ordinary CLI flags remain mandatory
//go:build desktop

package main

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/platform/system/strictjson"
)

const desktopLaunchProfileName = "alva-launch.json"

type desktopLaunchProfile struct {
	Version             int                                   `json:"version"`
	Arguments           []string                              `json:"arguments"`
	ExternalEnvironment map[string]desktopExternalEnvironment `json:"external_environment"`
}

type desktopExternalEnvironment struct {
	Path string `json:"PATH"`
}

func loadDesktopLaunchProfile(executable string, args []string) (desktopLaunchProfile, error) {
	launch := desktopLaunchProfile{Arguments: args}
	// An explicit terminal invocation never merges installed defaults.
	if len(args) != 0 {
		return launch, nil
	}
	canonical, err := filepath.EvalSymlinks(executable)
	if err != nil {
		return launch, errors.New("desktop executable unavailable")
	}
	macOS := filepath.Dir(canonical)
	contents := filepath.Dir(macOS)
	bundle := filepath.Dir(contents)
	if filepath.Base(macOS) != "MacOS" || filepath.Base(contents) != "Contents" || filepath.Ext(bundle) != ".app" {
		return launch, nil
	}
	manifest := filepath.Join(contents, "Resources", desktopLaunchProfileName)
	info, err := os.Lstat(manifest)
	if err != nil || !info.Mode().IsRegular() || info.Size() > 64<<10 {
		return launch, errors.New("installed desktop requires a regular Resources/alva-launch.json launch profile of at most 64 KiB")
	}
	file, err := os.Open(manifest)
	if err != nil {
		return launch, errors.New("installed desktop launch profile unavailable")
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, (64<<10)+1))
	if err != nil || len(data) > 64<<10 || strictjson.ValidateObject(data, &launch) != nil || json.Unmarshal(data, &launch) != nil || launch.Version != 1 || len(launch.Arguments) == 0 {
		return desktopLaunchProfile{}, errors.New("installed desktop launch profile requires strict version 1 JSON and explicit arguments")
	}
	for _, arg := range launch.Arguments {
		if !strings.HasPrefix(arg, "-") {
			continue
		}
		name, _, _ := strings.Cut(strings.TrimLeft(arg, "-"), "=")
		if name == "api-key-stdin" || name == "startup-secrets-stdin" {
			return desktopLaunchProfile{}, errors.New("installed desktop launch profile cannot request stdin secrets")
		}
	}
	return launch, nil
}

func applyDesktopLaunchEnvironment(external []bootstrap.ExternalEngine, environment map[string]desktopExternalEnvironment) error {
	for vendor, entry := range environment {
		if entry.Path == "" || strings.ContainsAny(entry.Path, "\x00\r\n") {
			return errors.New("installed desktop external PATH must contain explicit absolute directories")
		}
		for _, directory := range filepath.SplitList(entry.Path) {
			if !filepath.IsAbs(directory) {
				return errors.New("installed desktop external PATH must contain explicit absolute directories")
			}
		}
		found := false
		for i := range external {
			if external[i].Vendor == vendor && external[i].ApprovedHostAccess {
				external[i].Env = map[string]string{"PATH": entry.Path}
				found = true
			}
		}
		if !found {
			return errors.New("installed desktop external environment requires its corresponding approved executable")
		}
	}
	return nil
}
