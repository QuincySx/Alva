// INPUT: context, errors, agent/core Plugin and BeforeModel, kernel/abi session records, llm/wire, engine.Interjection
// OUTPUT: InterjectionPlugin: the BeforeModel middleware that commits interjected user messages at the next model boundary and reports their delivery
// POS: [L7 Alva product adapter] Interjection stays out of the kernel loop: a middleware appends the message to the native log and to this request, then the adapter emits InputDelivered
package alva

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// InterjectionMessagePrefix marks interjected user messages in the native log,
// so a fork cut counts only real prompts.
const InterjectionMessagePrefix = "interject:"

type interjectionPlugin struct {
	take      func() []engine.Interjection
	delivered func(engine.Interjection) error
}

// InterjectionPlugin returns the plugin a harness factory installs for an
// execution that can take interjections. take and delivered come from the
// engine.Request the adapter bound for this execution.
func InterjectionPlugin(take func() []engine.Interjection, delivered func(engine.Interjection) error) core.Plugin {
	return interjectionPlugin{take: take, delivered: delivered}
}

func (interjectionPlugin) Name() string { return "alva-interjection" }

func (p interjectionPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.take == nil || p.delivered == nil {
		return errors.New("interjection plugin requires its mailbox and receipt")
	}
	// Before ordinary model-request projections, so the message is part of the
	// request every later projection sees.
	return r.Middleware(core.BeforeModel("alva-interjection", p.beforeModel), -500)
}

// beforeModel commits the waiting messages after whatever the request already
// ends with (usually a tool result) and only then reports them delivered: a
// receipt never precedes the durable record.
func (p interjectionPlugin) beforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	items := p.take()
	if len(items) == 0 {
		return nil
	}
	if runtime.State == nil {
		return errors.New("interjection requires the bound native session")
	}
	batch := make([]abi.SessionRecord, 0, len(items))
	for _, item := range items {
		message := wire.TextMessage(wire.RoleUser, item.Text)
		message.ID = InterjectionMessagePrefix + item.ID
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
	}
	committed, err := runtime.State.Session().AppendRecords(ctx, batch)
	if err != nil {
		return err
	}
	if len(committed) != len(batch) {
		return errors.New("interjection session violated atomic append receipt")
	}
	for i, record := range committed {
		if record.Kind != abi.RecordConversation || record.Message == nil || record.Message.ID != batch[i].Message.ID {
			return errors.New("interjection session returned a mismatched message receipt")
		}
		request.Messages = append(request.Messages, wire.CloneMessages([]wire.Message{*record.Message})[0])
	}
	for _, item := range items {
		if err = p.delivered(item); err != nil {
			return err
		}
	}
	return nil
}
