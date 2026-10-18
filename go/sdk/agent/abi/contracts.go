// INPUT: context
// OUTPUT: Scope, SpawnRequest, SpawnService, Phase, Command
// POS: [L2.3 agent contracts] Agent-product vocabulary kept above the generic kernel execution contracts.
package abi

import (
	"context"
)

type Scope struct {
	ID       string
	ParentID string
}
type SpawnRequest struct {
	Scope     Scope
	Task      Task
	Workspace string
}
type SpawnService interface {
	Spawn(context.Context, SpawnRequest) (string, error)
	Cancel(context.Context, string) error
}
type Phase string

const (
	PhaseRunStart            Phase = "run_start"
	PhaseInputCommitted      Phase = "input_committed"
	PhaseIterationStart      Phase = "iteration_start"
	PhasePrepareModelRequest Phase = "prepare_model_request"
	PhaseModelCallStart      Phase = "model_call_start"
	PhaseBeforeModel         Phase = "before_model"
	PhaseAroundModel         Phase = "around_model"
	PhaseAfterModel          Phase = "after_model"
	PhaseModelCallEnd        Phase = "model_call_end"
	PhaseAssistantCommitted  Phase = "assistant_committed"
	PhaseToolBatchDeclared   Phase = "tool_batch_declared"
	PhaseToolBatchPlanned    Phase = "tool_batch_planned"
	PhaseToolUseDeclared     Phase = "tool_use_declared"
	PhasePrepareTool         Phase = "prepare_tool"
	PhaseBeforeTool          Phase = "before_tool"
	PhaseAroundTool          Phase = "around_tool"
	PhaseAfterTool           Phase = "after_tool"
	PhaseToolResultCommitted Phase = "tool_result_committed"
	PhaseToolBatchEnd        Phase = "tool_batch_end"
	PhaseAfterTurn           Phase = "after_turn"
	PhaseIterationEnd        Phase = "iteration_end"
	PhaseRunEnd              Phase = "run_end"
)

func (phase Phase) Valid() bool {
	switch phase {
	case PhaseRunStart, PhaseInputCommitted, PhaseIterationStart, PhasePrepareModelRequest, PhaseModelCallStart, PhaseBeforeModel, PhaseAroundModel, PhaseAfterModel, PhaseModelCallEnd, PhaseAssistantCommitted, PhaseToolBatchDeclared, PhaseToolBatchPlanned, PhaseToolUseDeclared, PhasePrepareTool, PhaseBeforeTool, PhaseAroundTool, PhaseAfterTool, PhaseToolResultCommitted, PhaseToolBatchEnd, PhaseAfterTurn, PhaseIterationEnd, PhaseRunEnd:
		return true
	}
	return false
}

type Command struct {
	Name        string
	Description string
}
