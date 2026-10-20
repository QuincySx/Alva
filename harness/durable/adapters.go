// INPUT: context, JSON/errors, interaction, agent/security and kernel/abi.
// OUTPUT: Durable approval/question handlers, typed resolution validation and explicit external requests.
// POS: [L5 optional harness] Binds approval once to an exact action and clarification once to an exact question across restarts.
package durable

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type approvalBinding struct {
	SessionID, ToolName, ArgumentsSHA256, InputSummary string
	Effect                                             abi.Effect
}
type approvalResult struct {
	Allow *bool `json:"allow"`
}

func decode(data []byte, out any) error {
	if len(data) == 0 || len(data) > 16<<10 || !wire.ValidJSON(data) {
		return errors.New("bounded valid JSON resolution required")
	}
	if err := strictjson.ValidateObject(data, out); err != nil {
		return err
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(out); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("one resolution value required")
	}
	return nil
}
func validateResolution(r Request, result json.RawMessage) error {
	if len(result) == 0 || len(result) > 16<<10 || !wire.ValidJSON(result) {
		return errors.New("bounded valid JSON result required")
	}
	switch r.Kind {
	case "approval":
		var decision approvalResult
		if err := decode(result, &decision); err != nil {
			return err
		}
		if decision.Allow == nil {
			return errors.New("explicit allow boolean required")
		}
	case "question":
		var q interaction.Question
		var answer interaction.Answer
		if err := json.Unmarshal(r.Payload, &q); err != nil {
			return err
		}
		if err := decode(result, &answer); err != nil {
			return err
		}
		if _, err := q.Resolve(answer); err != nil {
			return err
		}
	}
	return nil
}

func validateRequestSource(r Request) error {
	switch r.Kind {
	case "question":
		var q interaction.Question
		if err := json.Unmarshal(r.Payload, &q); err != nil {
			return err
		}
		if err := q.Validate(); err != nil {
			return err
		}
		if q.RunID != r.RunID || q.CallID != r.CallID {
			return errors.New("question source identity differs from durable request")
		}
	case "approval":
		var binding approvalBinding
		if err := json.Unmarshal(r.Payload, &binding); err != nil {
			return err
		}
		if binding.SessionID == "" || binding.ToolName == "" || len(binding.ArgumentsSHA256) != 64 {
			return errors.New("approval source binding missing")
		}
	case "external":
		var object map[string]json.RawMessage
		if err := json.Unmarshal(r.Payload, &object); err != nil || object == nil {
			return errors.New("external request object missing")
		}
	}
	return nil
}
func (c *Controller) enqueue(ctx context.Context, kind, callID string, payload json.RawMessage) (Request, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.driver == nil || c.runID == "" || callID == "" {
		return Request{}, errors.New("durable request requires actual active invocation")
	}
	s, err := c.load(ctx)
	if err != nil {
		return Request{}, err
	}
	if s.State.Status != Running {
		return Request{}, errors.New("durable session is not executing")
	}
	if len(s.State.Requests) >= 16 {
		return Request{}, errors.New("durable request admission exceeds 16")
	}
	r, err := newRequest(kind, c.runID, callID, payload, c.config.Now().Add(c.config.TTL))
	if err != nil {
		return Request{}, err
	}
	s.State.Requests = append(s.State.Requests, r)
	_, err = c.config.Backend.Commit(ctx, s.Revision, s.State, nil)
	return r, err
}
func (c *Controller) consume(ctx context.Context, kind string, payload json.RawMessage) (json.RawMessage, bool, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.driver == nil {
		return nil, false, errors.New("durable handler requires actual Controller execution")
	}
	s, err := c.load(ctx)
	if err != nil {
		return nil, false, err
	}
	key := fingerprint(kind, payload)
	for i, r := range s.State.Grants {
		if r.Kind == kind && r.Binding == key {
			if !c.config.Now().Before(r.ExpiresAt) {
				return nil, false, ErrExpired
			}
			s.State.Grants = append(s.State.Grants[:i], s.State.Grants[i+1:]...)
			_, err = c.config.Backend.Commit(ctx, s.Revision, s.State, nil)
			return r.Result, true, err
		}
	}
	return nil, false, nil
}

// Approve is the normal security.ApprovalHandler port. A restored decision is
// consumed durably BEFORE execution and matches session/tool/effect/raw arguments.
// Reissuing a tool is an explicit new model decision, never host-side replay.
func (c *Controller) Approve(ctx context.Context, r security.ApprovalRequest) (security.ApprovalDecision, error) {
	c.mu.Lock()
	valid := c.driver != nil && r.SessionID == c.config.Backend.Log().ID() && r.RunID == c.runID
	c.mu.Unlock()
	if !valid || r.CallID == "" || len(r.ArgumentsSHA256) != 64 {
		return security.ApprovalDecision{}, errors.New("approval lacks actual session/run/call/argument binding")
	}
	payload, err := json.Marshal(approvalBinding{r.SessionID, r.ToolName, r.ArgumentsSHA256, r.InputSummary, r.Effect})
	if err != nil {
		return security.ApprovalDecision{}, err
	}
	result, found, err := c.consume(ctx, "approval", payload)
	if err != nil {
		return security.ApprovalDecision{}, err
	}
	if found {
		var value approvalResult
		if err := decode(result, &value); err != nil || value.Allow == nil {
			return security.ApprovalDecision{}, errors.New("invalid restored approval")
		}
		return security.ApprovalDecision{Allowed: *value.Allow, Scope: security.Once}, nil
	}
	if _, err := c.enqueue(ctx, "approval", r.CallID, payload); err != nil {
		return security.ApprovalDecision{}, err
	}
	return security.ApprovalDecision{}, &suspension{}
}
func questionPayload(q interaction.Question) json.RawMessage {
	// The source retains actual identity. fingerprint normalizes only the
	// comparison descriptor, never the stored evidence.
	data, _ := json.Marshal(q)
	return data
}
func (c *Controller) Ask(ctx context.Context, q interaction.Question) (interaction.Answer, error) {
	if err := q.Validate(); err != nil {
		return interaction.Answer{}, err
	}
	c.mu.Lock()
	valid := c.driver != nil && q.SessionID == c.config.Backend.Log().ID() && q.RunID == c.runID
	c.mu.Unlock()
	if !valid {
		return interaction.Answer{}, errors.New("question belongs to another active invocation")
	}
	payload := questionPayload(q)
	result, found, err := c.consume(ctx, "question", payload)
	if err != nil {
		return interaction.Answer{}, err
	}
	if found {
		var answer interaction.Answer
		err := decode(result, &answer)
		return answer, err
	}
	if _, err := c.enqueue(ctx, "question", q.CallID, payload); err != nil {
		return interaction.Answer{}, err
	}
	return interaction.Answer{}, &suspension{}
}
