// INPUT: Saved product workspace, the confined managed-skill repository and a form's name/description/body
// OUTPUT: SkillForm, SkillFormFor, SaveSkillForm and ComposeSkill
// POS: [L7 Hub product] Form editing of managed Skills; an edit archives the old version first and restores it on failure
package product

import (
	"context"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/protocol/skill"
	"go.yaml.in/yaml/v3"
)

// SkillForm is the editable part of a managed Skill. Other frontmatter of an
// edited Skill (license, tools, metadata) is kept unchanged.
type SkillForm struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Body        string `json:"body"`
	Explicit    bool   `json:"explicit"`
}

// ComposeSkill renders SKILL.md and validates it with the shared parser.
func ComposeSkill(meta skill.Meta, body string) (string, error) {
	front, err := yaml.Marshal(meta)
	if err != nil {
		return "", err
	}
	raw := "---\n" + string(front) + "---\n\n" + strings.TrimSpace(body) + "\n"
	if _, err = skill.Parse([]byte(raw)); err != nil {
		return "", err
	}
	return raw, nil
}

func (f SkillForm) problems() FieldErrors {
	var problems FieldErrors
	if !skill.ValidName(f.Name) {
		problems = append(problems, FieldError{"skill.name", "名称只能用小写字母、数字和连字符，最长 64 个字符，不能以连字符开头或结尾。"})
	}
	if strings.TrimSpace(f.Description) == "" || len(f.Description) > 1024 || strings.ContainsAny(f.Description, "<>") || !utf8.ValidString(f.Description) {
		problems = append(problems, FieldError{"skill.description", "请填写说明（最长 1024 字节，不能包含 < 或 >），Alva 靠它决定何时使用这个技能。"})
	}
	if strings.TrimSpace(f.Body) == "" || len(f.Body) > skill.MaxDocumentBytes-4096 || !utf8.ValidString(f.Body) {
		problems = append(problems, FieldError{"skill.body", "请填写技能正文（最大约 1 MiB）。"})
	}
	return problems
}

// SkillFormFor loads one managed Skill into the form.
func (s *HubService) SkillFormFor(ctx context.Context, id, name string) (SkillForm, error) {
	document, err := s.SkillDocument(ctx, id, name)
	if err != nil {
		return SkillForm{}, err
	}
	return SkillForm{Name: document.Meta.Name, Description: document.Meta.Description, Body: document.Body, Explicit: document.Meta.Invocation == skill.Explicit}, nil
}

// SaveSkillForm installs a new Skill (original empty) or replaces original:
// the old version, with its resource files, is archived first and restored
// if the new install fails. It never overwrites a different existing Skill.
func (s *HubService) SaveSkillForm(ctx context.Context, id, original string, form SkillForm) error {
	form.Name, form.Description = strings.TrimSpace(form.Name), strings.TrimSpace(form.Description)
	if problems := form.problems(); len(problems) != 0 {
		return problems
	}
	invocation := skill.Auto
	if form.Explicit {
		invocation = skill.Explicit
	}
	if original == "" {
		raw, err := ComposeSkill(skill.Meta{Name: form.Name, Description: form.Description, Invocation: invocation}, form.Body)
		if err != nil {
			return FieldErrors{{"skill", "技能内容无效：" + err.Error()}}
		}
		if err = s.InstallSkill(ctx, id, raw); err != nil {
			return FieldErrors{{"skill.name", "无法安装（可能已有同名技能）：" + err.Error()}}
		}
		return nil
	}
	repository, err := s.projectSkills(ctx, id, true, false)
	if err != nil {
		return err
	}
	document, err := repository.Load(ctx, original)
	if err != nil {
		repository.Close()
		return err
	}
	meta := document.Meta
	meta.Name, meta.Description, meta.Invocation = form.Name, form.Description, invocation
	raw, err := ComposeSkill(meta, form.Body)
	if err != nil {
		repository.Close()
		return FieldErrors{{"skill", "技能内容无效：" + err.Error()}}
	}
	files := map[string][]byte{}
	paths, err := repository.Resources(ctx, original)
	for _, path := range paths {
		if err != nil || path == "SKILL.md" {
			continue
		}
		resource, readErr := repository.Resource(ctx, original, path)
		if readErr != nil {
			err = readErr
			break
		}
		files[path] = resource.Bytes
	}
	repository.Close()
	if err != nil {
		return err
	}
	files["SKILL.md"] = []byte(raw)
	receipt, err := s.ArchiveSkill(ctx, id, original)
	if err != nil {
		return err
	}
	repository, err = s.projectSkills(ctx, id, true, false)
	if err == nil {
		err = repository.Install(ctx, form.Name, files)
		repository.Close()
	}
	if err != nil {
		if restoreErr := s.RestoreSkill(ctx, id, receipt); restoreErr != nil {
			return errors.Join(err, errors.New("旧版本已归档但未能自动恢复，可在列表中手动恢复："+receipt))
		}
		return FieldErrors{{"skill.name", "保存失败，已恢复旧版本：" + err.Error()}}
	}
	return nil
}
