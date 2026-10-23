// INPUT: Explicit native directory selection and the unsubmitted new-task workspace draft.
// OUTPUT: A selected project folder for the existing HubService.Create path.
// POS: [L7 Hub UI] Selecting a directory grants no tool permission and never retargets an existing session.
//go:build desktop

package hubdesktop

import "github.com/egoist/mygo"

func (p *NativePage) chooseWorkspace() {
	if p.selected != "" || p.busy || p.window == nil {
		return
	}
	p.busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "选择项目文件夹", Directory: true})
		p.dispatch(func() {
			p.busy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			if len(paths) > 0 && p.selected == "" {
				p.workspace = paths[0]
				p.problem = ""
			}
		})
	}()
}
