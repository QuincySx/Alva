// INPUT: Optional --page <component>, --dark and --narrow flags.
// OUTPUT: A native window showing Alva Kit's variant matrices; no storage, network, model or session.
// POS: [L7 Kit] Visual review entry for apps/ui/kit; a main package like every other cmd directory, desktop-tagged because it draws with MyGo.
//go:build desktop

package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/QuincySx/Alva/apps/ui/kit/gallery"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// prepare parses flags into the gallery state; it opens no window.
func prepare(args []string, stderr io.Writer) (*gallery.App, error) {
	flags := flag.NewFlagSet("alva-kit-gallery", flag.ContinueOnError)
	flags.SetOutput(stderr)
	page := flags.String("page", "", "component page to open (for example Button)")
	dark := flags.Bool("dark", false, "start in the dark appearance")
	narrow := flags.Bool("narrow", false, "start with the Narrow viewport")
	if err := flags.Parse(args); err != nil {
		return nil, err
	}
	if flags.NArg() != 0 {
		return nil, fmt.Errorf("unexpected arguments: %v", flags.Args())
	}
	app := gallery.New()
	if *page != "" && !app.Select(*page) {
		return nil, fmt.Errorf("no gallery page %q", *page)
	}
	app.Dark, app.Narrow = *dark, *narrow
	return app, nil
}

func main() {
	app, err := prepare(os.Args[1:], os.Stderr)
	if err != nil {
		os.Exit(2)
	}
	if err = theme.RegisterFonts(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	mygo.App.WhenReady(func() {
		mygo.NewWindow(mygo.WindowOptions{Title: "Alva Kit", Width: 1440, Height: 960, Content: ui.View(app.View)})
	})
	if err = mygo.App.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
