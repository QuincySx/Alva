// INPUT: Saved product workspace and explicit user managed-skill operations
// OUTPUT: Confined project skills list/load/install/archive/restore with durable archive receipts
// POS: [L7 Hub product] No global scanning, downloads, script execution or changes to a running Agent
package product

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/integration/protocol/skill"
	"os"
)

const ManagedSkillDirectory = ".alva/skills"

func (s *HubService) projectSkills(ctx context.Context, id string, write, create bool) (*skill.FS, error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	if write && detail.Session.Status == hub.Running {
		return nil, errors.New("finish current Turn before editing project skills")
	}
	if create {
		root, err := os.OpenRoot(detail.Session.Workspace)
		if err != nil {
			return nil, err
		}
		defer root.Close()
		for _, relative := range []string{".alva", ManagedSkillDirectory} {
			info, err := root.Lstat(relative)
			if errors.Is(err, os.ErrNotExist) {
				if err = root.Mkdir(relative, 0700); err != nil {
					return nil, err
				}
				continue
			}
			if err != nil {
				return nil, err
			}
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				return nil, errors.New("managed project skill ancestors must be real directories")
			}
		}
	}
	return skill.OpenWithin(detail.Session.Workspace, ManagedSkillDirectory, write)
}
func (s *HubService) InitializeSkills(ctx context.Context, id string) error {
	repository, err := s.projectSkills(ctx, id, true, true)
	if err != nil {
		return err
	}
	return repository.Close()
}
func (s *HubService) Skills(ctx context.Context, id string) ([]skill.Skill, error) {
	repository, err := s.projectSkills(ctx, id, false, false)
	if err != nil {
		return nil, err
	}
	defer repository.Close()
	return repository.List(ctx)
}
func (s *HubService) SkillDocument(ctx context.Context, id, name string) (skill.Document, error) {
	repository, err := s.projectSkills(ctx, id, false, false)
	if err != nil {
		return skill.Document{}, err
	}
	defer repository.Close()
	return repository.Load(ctx, name)
}
func (s *HubService) SkillRaw(ctx context.Context, id, name string) (string, error) {
	repository, err := s.projectSkills(ctx, id, false, false)
	if err != nil {
		return "", err
	}
	defer repository.Close()
	resource, err := repository.Resource(ctx, name, "SKILL.md")
	return string(resource.Bytes), err
}
func (s *HubService) InstallSkill(ctx context.Context, id, raw string) error {
	document, err := skill.Parse([]byte(raw))
	if err != nil {
		return err
	}
	repository, err := s.projectSkills(ctx, id, true, false)
	if err != nil {
		return err
	}
	defer repository.Close()
	return repository.Install(ctx, document.Meta.Name, map[string][]byte{"SKILL.md": []byte(raw)})
}
func (s *HubService) ArchiveSkill(ctx context.Context, id, name string) (string, error) {
	repository, err := s.projectSkills(ctx, id, true, false)
	if err != nil {
		return "", err
	}
	defer repository.Close()
	receipt, err := repository.Archive(ctx, name)
	if err != nil {
		return "", err
	}
	if err = s.Store.RecordSkillArchive(ctx, id, receipt); err != nil {
		return receipt, err
	}
	return receipt, nil
}
func (s *HubService) RestoreSkill(ctx context.Context, id, receipt string) error {
	value, err := s.Store.Settings(ctx, id)
	if err != nil {
		return err
	}
	owned := false
	for _, saved := range value.SkillArchives {
		owned = owned || saved == receipt
	}
	if !owned {
		return errors.New("skill archive receipt does not belong to this product")
	}
	repository, err := s.projectSkills(ctx, id, true, false)
	if err != nil {
		return err
	}
	defer repository.Close()
	return repository.Restore(ctx, receipt)
}
