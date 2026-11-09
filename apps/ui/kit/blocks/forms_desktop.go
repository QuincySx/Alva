// INPUT: Alva ProviderForm (Preset/Custom), ModelForm (Add/Edit) and ProviderModels (six catalog states) definitions.
// OUTPUT: Form shells and the model catalog panel: layout, titles, footers and states around fields the caller supplies.
// POS: [L7 Kit blocks] Validation, saving and fetching are the product's; callbacks say what the user asked for, and the shell keeps its buttons from firing twice while busy.
//go:build desktop

package blocks

import (
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

const formWidth = 480

// ProviderSource is where a provider's settings come from; Preset is the zero value.
type ProviderSource uint8

const (
	SourcePreset ProviderSource = iota
	SourceCustom
)

// ProviderFormView is the data of ProviderForm.
type ProviderFormView struct {
	Source ProviderSource
	// Busy is set while saving: the save action shows busy and does not fire again.
	Busy bool
	// Message is a form-level error under the fields.
	Message string
}

// ProviderFormActions are the callbacks of ProviderForm.
type ProviderFormActions struct {
	// OnSource switches between preset and custom; OnSave and OnCancel end the form.
	OnSource         func(ProviderSource)
	OnSave, OnCancel func()
}

// busyOff marks both segments disabled while the form is busy.
func busyOff(busy bool) []bool {
	if busy {
		return []bool{true, true}
	}
	return nil
}

// ProviderForm is the connection form shell: a source switch, the caller's fields and a footer. A preset needs only the
// fields the caller passes for it; a custom source passes more.
func ProviderForm(c *ui.Context, v ProviderFormView, a ProviderFormActions, fields func(), key string) ui.Element {
	audit.Named("ProviderForm", "连接表单")
	source := int(v.Source)
	return kit.Card(c, key, func() {
		kit.Text(c, "连接", kit.TextProps{Role: kit.RoleSubtitle, Strong: true, Key: "title"})
		// While busy (saving) the whole form is read-only: the source switch and every field the caller built.
		kit.Segmented(c, &source, []string{"预设", "自定义"}, kit.SegmentedProps{Label: "连接来源", Disabled: busyOff(v.Busy), Key: "source"})
		if source != int(v.Source) && a.OnSource != nil && !v.Busy {
			a.OnSource(ProviderSource(source))
		}
		kit.Field(c, kit.FieldProps{Disabled: v.Busy, Key: "fields"}, func() {
			ui.Column(c.Key("fields-body")).FillWidth().Gap(theme.Space3).Children(fields)
		})
		if v.Message != "" {
			kit.Text(c, v.Message, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneDanger, Key: "message"})
		}
		kit.DialogFooter(c, func() {
			kit.Button(c, "取消", kit.ButtonProps{Disabled: v.Busy, OnClick: a.OnCancel, Key: "cancel"})
			kit.Button(c, "保存", kit.ButtonProps{Kind: kit.Primary, Busy: v.Busy, OnClick: a.OnSave, Key: "save"})
		})
	}).MaxWidth(formWidth)
}

// ModelFormMode is whether the model form adds or edits; Add is the zero value.
type ModelFormMode uint8

const (
	ModelAdd ModelFormMode = iota
	ModelEdit
)

// ModelFormView is the data of ModelForm. ID and Name are required and controlled by the caller; nil ones do not panic
// (locals are used) but are reported to the kit audit.
type ModelFormView struct {
	Mode     ModelFormMode
	ID, Name *string
	Busy     bool
	Message  string
}

// ModelFormActions are the callbacks of ModelForm.
type ModelFormActions struct{ OnSave, OnCancel func() }

// ModelForm is the model add and edit form shell. The ID is editable when adding and read-only when editing.
func ModelForm(c *ui.Context, v ModelFormView, a ModelFormActions, extra func(), key string) ui.Element {
	audit.Named("ModelForm", "模型表单")
	if v.ID == nil || v.Name == nil {
		// ID and Name are required; locals keep the frame alive (edits are lost) and the audit reports the mistake.
		audit.Named("ModelForm: ModelFormView.ID and Name are required", "")
		localID, localName := "", ""
		if v.ID == nil {
			v.ID = &localID
		}
		if v.Name == nil {
			v.Name = &localName
		}
	}
	title := "添加模型"
	if v.Mode == ModelEdit {
		title = "编辑模型"
	}
	return kit.Card(c, key, func() {
		kit.Text(c, title, kit.TextProps{Role: kit.RoleSubtitle, Strong: true, Key: "title"})
		kit.Field(c, kit.FieldProps{Title: "模型 ID", ReadOnly: v.Mode == ModelEdit, Disabled: v.Busy, Hint: map[bool]string{true: "已添加的模型不能改 ID"}[v.Mode == ModelEdit], Key: "id"}, func() {
			kit.Input(c, v.ID, kit.InputProps{Placeholder: "例如 my-model", Key: "id-input"})
		})
		kit.Field(c, kit.FieldProps{Title: "显示名称", Disabled: v.Busy, Key: "name"}, func() {
			kit.Input(c, v.Name, kit.InputProps{Placeholder: "显示在选择器里", Key: "name-input"})
		})
		if extra != nil {
			kit.Field(c, kit.FieldProps{Disabled: v.Busy, Key: "extra"}, func() { ui.Column(c.Key("extra-body")).FillWidth().Gap(theme.Space3).Children(extra) })
		}
		if v.Message != "" {
			kit.Text(c, v.Message, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneDanger, Key: "message"})
		}
		kit.DialogFooter(c, func() {
			kit.Button(c, "取消", kit.ButtonProps{Disabled: v.Busy, OnClick: a.OnCancel, Key: "cancel"})
			kit.Button(c, "保存", kit.ButtonProps{Kind: kit.Primary, Busy: v.Busy, OnClick: a.OnSave, Key: "save"})
		})
	}).MaxWidth(formWidth)
}

// CatalogState is the state of a provider's model catalog; Idle is the zero value (not fetched yet).
type CatalogState uint8

const (
	CatalogIdle CatalogState = iota
	CatalogLoading
	CatalogReady
	CatalogEmpty
	CatalogUnavailable
	CatalogError
)

// ProviderModelsView is the data of ProviderModels.
type ProviderModelsView struct {
	State  CatalogState
	Models []ModelRowView
	// Message explains Unavailable and Error.
	Message string
}

// ProviderModelsActions are the callbacks of ProviderModels.
type ProviderModelsActions struct {
	// OnFetch asks for the catalog (also the retry); it does not fire while loading.
	OnFetch func()
	// Row builds the actions of one model row.
	Row func(index int) ModelRowActions
}

// ProviderModels shows a provider's model catalog in one of its six states.
func ProviderModels(c *ui.Context, v ProviderModelsView, a ProviderModelsActions, key string) ui.Element {
	audit.Named("ProviderModels", "模型目录")
	return ui.Column(keyed(c, key)).FillWidth().Gap(theme.Space2).Label("模型目录").Children(func() {
		ui.Row(c.Key("head")).FillWidth().AlignItems(ui.Center).Children(func() {
			kit.Text(c, "模型", kit.TextProps{Strong: true, Key: "title"}).Grow(1)
			kit.Button(c, "获取模型", kit.ButtonProps{Kind: kit.Ghost, Icon: icon.Refresh, Busy: v.State == CatalogLoading, Disabled: v.State == CatalogUnavailable, OnClick: a.OnFetch, Key: "fetch"})
		})
		switch v.State {
		case CatalogLoading:
			kit.SkeletonGroup(c, "loading", func() {
				for i := range 3 {
					kit.Skeleton(c.Key(i), formWidth, theme.Space6)
				}
			})
		case CatalogEmpty:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyNone, Title: "没有可用的模型", Description: v.Message, Key: "state"})
		case CatalogUnavailable:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyUnavailable, Title: "暂时无法获取", Description: v.Message, Key: "state"})
		case CatalogError:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyError, Title: "获取失败", Description: v.Message, Key: "state"})
		case CatalogReady:
			for i, m := range v.Models {
				var actions ModelRowActions
				if a.Row != nil {
					actions = a.Row(i)
				}
				ModelRow(c, m, ModelManage, actions, "model-"+itoa(i))
			}
		default:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyNone, Title: "还没有获取模型", Description: "点击「获取模型」从供应商读取目录", Key: "state"})
		}
	})
}
