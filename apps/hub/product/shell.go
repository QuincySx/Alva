// INPUT: Committed product branches, actual application shell owner and opaque already-owned job IDs
// OUTPUT: Cold branch-scoped job metadata, explicit physical Stop and idle owner reset
// POS: [L7 Hub product] Native/HTTP controls never accept a native session/PID as authority or execute a new command.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/extension/shell"
)

type ShellJob struct {
	BranchID string          `json:"branch_id"`
	Job      shell.JobStatus `json:"job"`
}

func (s *HubService) ShellJobs(ctx context.Context, id string) ([]ShellJob, error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	jobs := []ShellJob{}
	if s.Shells == nil {
		return jobs, nil
	}
	for _, branch := range detail.Branches {
		if branch.Target.EngineID != "alva" || branch.NativeSessionID == "" {
			continue
		}
		for _, job := range s.Shells.List(id, branch.NativeSessionID) {
			jobs = append(jobs, ShellJob{BranchID: branch.ID, Job: job})
		}
	}
	return jobs, nil
}
func (s *HubService) StopShellJob(ctx context.Context, id, branchID, job string) error {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if s.Shells == nil {
		return errors.New("application shell owner unavailable")
	}
	for _, branch := range detail.Branches {
		if branch.ID == branchID && branch.Target.EngineID == "alva" && branch.NativeSessionID != "" {
			return s.Shells.Stop(ctx, id, branch.NativeSessionID, job)
		}
	}
	return errors.New("owned Alva branch required")
}
func (s *HubService) ResetShellOwner(ctx context.Context, id, branchID string) error {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if s.Shells == nil {
		return errors.New("application shell owner unavailable")
	}
	if detail.Session.Status == hub.Running {
		return errors.New("finish or Stop actual Turn before resetting its shell owner")
	}
	for _, branch := range detail.Branches {
		if branch.ID == branchID && branch.Target.EngineID == "alva" && branch.NativeSessionID != "" {
			return s.Shells.Reset(ctx, id, branch.NativeSessionID)
		}
	}
	return errors.New("owned Alva branch required")
}
