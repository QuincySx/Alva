// INPUT: Actual product/native session, approved optional settings, shared guard/workspace/locks and risk reporter
// OUTPUT: Optional real shell/web/planning/notebook/interaction/utility Plugin assembly
// POS: [L7 Hub product] One native recipe for desktop/server with exact tools and owned resource cleanup.
package bootstrap

import (
	"context"
	"errors"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/extension/notebook"
	"github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/extension/shell"
	"github.com/QuincySx/Alva/extension/utility"
	"github.com/QuincySx/Alva/extension/web"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func (a *Application) optionalTools(ctx context.Context, owner, native, cwd string, s hub.ToolSettings, w *workspacefs.Workspace, g *security.Guard, locks *core.ToolLockRegistry, report func(context.Context, string, string, string) error, retainWorkspace func() (func(), error), plugins *[]core.Plugin, trusted *[]abi.Tool, resources *[]attemptResource) error {
	add := func(p core.Plugin, tools []abi.Tool) {
		*plugins = append(*plugins, p)
		*trusted = append(*trusted, tools...)
	}
	if s.Planning {
		p, err := planning.New(ctx, planning.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g})
		if err != nil {
			return err
		}
		add(p, p.Tools())
	}
	if s.Utility {
		p, err := utility.New(ctx, utility.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g})
		if err != nil {
			return err
		}
		add(p, p.Tools())
	}
	if s.Interaction {
		p, err := interaction.New(ctx, interaction.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g, Handler: a.Questions})
		if err != nil {
			return err
		}
		add(p, p.Tools())
	}
	if s.Notebook {
		p, err := notebook.New(ctx, notebook.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g, Workspace: w, Locks: locks, Report: func(ctx context.Context, v notebook.Activity) error { return report(ctx, v.SessionID, v.ID, v.Name) }})
		if err != nil {
			return err
		}
		add(p, p.Tools())
	}
	if s.Shell.Enabled {
		if retainWorkspace == nil {
			return errors.New("Hub shell requires the actual admitted product workspace retention capability")
		}
		p, err := a.Shells.Bind(ctx, shell.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g, Locks: locks, Executable: s.Shell.Executable, Env: s.Shell.Env, ApprovedHostAccess: s.Shell.HostAccessApproved, RetainWorkspace: retainWorkspace, Report: func(ctx context.Context, v shell.Activity) error { return report(ctx, v.SessionID, v.ID, v.Name) }})
		if err != nil {
			return err
		}
		*resources = append(*resources, p)
		add(p, p.Tools())
	}
	if s.Web.Enabled {
		c := web.Config{Repository: a.Repository, Owner: owner, SessionID: native, Cwd: cwd, Guard: g, ReadURLs: s.Web.ReadURLs, ApprovedNetworkAccess: s.Web.NetworkAccessApproved, Timeout: time.Duration(s.Web.TimeoutMS) * time.Millisecond, Network: web.NetworkPolicy{AllowedPrivateOrigins: s.Web.AllowedPrivateOrigins}, Report: func(ctx context.Context, v web.Activity) error { return report(ctx, v.SessionID, v.ID, v.Name) }}
		if s.Web.SearchEndpoint != "" {
			c.Search = &web.SearchConfig{Kind: "duckduckgo_instant", Endpoint: s.Web.SearchEndpoint}
		}
		p, err := web.New(ctx, c)
		if err != nil {
			return err
		}
		*resources = append(*resources, p)
		add(p, p.Tools())
	}
	return nil
}
