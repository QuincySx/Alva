// INPUT: Pure fake fifth engine and independent engine declarations
// OUTPUT: Extensible registry, detached descriptions, conservative runtime registration and no discovery/execution regressions
// POS: [L7 Hub product] Reading capabilities is cold and cannot replace an engine behind durable native bindings.
package hub

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/go/integration/engine"
)

type descriptorRuntime struct{ calls int }

func (r *descriptorRuntime) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	r.calls++
	return nil
}
func (*descriptorRuntime) Cancel(context.Context, string) error { return nil }
func (*descriptorRuntime) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (*descriptorRuntime) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true}
}

func TestFifthEngineDescriptorIsColdDetachedAndCannotReplaceBindings(t *testing.T) {
	r := New(nil)
	if r.Register("typed-nil", (*descriptorRuntime)(nil)) == nil {
		t.Fatal("typed-nil runtime admitted")
	}
	fifth := &descriptorRuntime{}
	d := engine.DescribeRuntime(fifth)
	d.PermissionPlans = []engine.PermissionPlan{{ProfileID: "read", Relation: engine.PermissionStricter, Summary: "explicit fifth-engine declaration"}}
	if err := r.RegisterDescriptor("fifth", d); err != nil {
		t.Fatal(err)
	}
	d.PermissionPlans[0].ProfileID = "write"
	got, ok := r.Descriptor("fifth")
	if !ok || !got.CanSatisfy("read") || got.CanSatisfy("write") || got.CanSatisfy("") {
		t.Fatal("registry retained mutable permission declaration")
	}
	got.PermissionPlans[0].ProfileID = "write"
	descriptions := r.EngineDescriptions()
	if len(descriptions) != 1 || descriptions[0].PermissionPlans[0].ProfileID != "read" || descriptions[0].Inspection.Level != engine.InspectionUnsupported || fifth.calls != 0 {
		t.Fatal("cold descriptor mutated authority or executed engine", descriptions)
	}
	descriptions[0].PermissionPlans[0].ProfileID = "write"
	if d, _ := r.Descriptor("fifth"); !d.CanSatisfy("read") {
		t.Fatal("public descriptions shared registry memory")
	}
	if r.Register("fifth", &descriptorRuntime{}) == nil {
		t.Fatal("engine replaced behind native bindings")
	}
	bad := engine.DescribeRuntime(fifth)
	bad.PermissionPlans = []engine.PermissionPlan{{ProfileID: "unsafe", Relation: "weaker"}}
	if bad.CanSatisfy("unsafe") || r.RegisterDescriptor("bad", bad) == nil {
		t.Fatal("unproven permission relation accepted")
	}
	if err := r.Register("conservative", fifth); err != nil {
		t.Fatal(err)
	}
	if d, _ := r.Descriptor("conservative"); d.Inspector != nil || len(d.PermissionPlans) != 0 || d.Config.SupportsMaxTurns {
		t.Fatal("runtime-only registration invented capabilities")
	}
	if err := r.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if r.Register("late", fifth) == nil {
		t.Fatal("closed registry accepted engine")
	}
}
