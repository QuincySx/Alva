// INPUT: context, errors, fmt
// OUTPUT: StateGraph, Node, NodeResult, SendTo, CompiledGraph, Start, End
// POS: [L3 capability library] Typed graph compilation independent from the standard AgentLoop and Plugin runtime.
package graph

import (
	"context"
	"errors"
	"fmt"
)

const (
	Start = "__start__"
	End   = "__end__"
)

type ResultKind string

const (
	ResultUpdate ResultKind = "update"
	ResultSends  ResultKind = "sends"
)

type SendTo[S any] struct {
	Node  string
	State S
}
type NodeResult[S any] struct {
	Kind  ResultKind
	State S
	Sends []SendTo[S]
}

func Update[S any](state S) NodeResult[S] { return NodeResult[S]{Kind: ResultUpdate, State: state} }
func Sends[S any](sends ...SendTo[S]) NodeResult[S] {
	return NodeResult[S]{Kind: ResultSends, Sends: sends}
}

type Node[S any] func(context.Context, S) (NodeResult[S], error)
type Router[S any] func(context.Context, S) (string, error)
type Merge[S any] func(S, []S) (S, error)
type edge[S any] struct {
	from, to string
	router   Router[S]
}
type StateGraph[S any] struct {
	nodes map[string]Node[S]
	edges []edge[S]
	entry string
	clone func(S) S
	merge Merge[S]
}

func NewStateGraph[S any](clone func(S) S) *StateGraph[S] {
	return &StateGraph[S]{nodes: make(map[string]Node[S]), clone: clone}
}
func (builder *StateGraph[S]) AddNode(name string, node Node[S]) error {
	if name == "" || name == Start || name == End || node == nil {
		return errors.New("invalid graph node identity/function")
	}
	if _, exists := builder.nodes[name]; exists {
		return fmt.Errorf("duplicate graph node %q", name)
	}
	builder.nodes[name] = node
	return nil
}
func (builder *StateGraph[S]) AddEdge(from, to string) {
	builder.edges = append(builder.edges, edge[S]{from: from, to: to})
}
func (builder *StateGraph[S]) AddConditionalEdge(from string, router Router[S]) {
	builder.edges = append(builder.edges, edge[S]{from: from, router: router})
}
func (builder *StateGraph[S]) SetEntryPoint(name string) { builder.entry = name }
func (builder *StateGraph[S]) SetMerge(merge Merge[S])   { builder.merge = merge }
func (builder *StateGraph[S]) Compile() (*CompiledGraph[S], error) {
	if builder.clone == nil {
		return nil, errors.New("graph requires explicit state ownership clone")
	}
	if builder.entry == "" || builder.nodes[builder.entry] == nil {
		return nil, fmt.Errorf("graph entry point %q is not registered", builder.entry)
	}
	for _, edge := range builder.edges {
		if edge.from != Start && builder.nodes[edge.from] == nil {
			return nil, fmt.Errorf("graph edge source %q not registered", edge.from)
		}
		if edge.router == nil && edge.to != End && builder.nodes[edge.to] == nil {
			return nil, fmt.Errorf("graph edge target %q not registered", edge.to)
		}
	}
	nodes := make(map[string]Node[S], len(builder.nodes))
	for name, node := range builder.nodes {
		nodes[name] = node
	}
	return &CompiledGraph[S]{nodes: nodes, edges: append([]edge[S](nil), builder.edges...), entry: builder.entry, clone: builder.clone, merge: builder.merge}, nil
}

type CompiledGraph[S any] struct {
	nodes map[string]Node[S]
	edges []edge[S]
	entry string
	clone func(S) S
	merge Merge[S]
}
