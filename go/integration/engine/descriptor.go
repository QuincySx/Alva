// INPUT: Engine Runtime/Inspector contracts and explicit declarative assembly metadata
// OUTPUT: Independent Descriptor, immutable public description and conservative configuration/permission/install surfaces
// POS: [L4.5 engine contract] Capability declaration does not discover executables, read accounts or authorize execution.
package engine

import (
	"errors"
	"reflect"
	"strings"
	"unicode/utf8"
)

type PermissionRelation string

const (
	PermissionEquivalent PermissionRelation = "equivalent"
	PermissionStricter   PermissionRelation = "stricter"
)

type PermissionPlan struct {
	ProfileID string             `json:"profile_id"`
	Relation  PermissionRelation `json:"relation"`
	Summary   string             `json:"summary"`
}
type ConfigSurface struct {
	SupportsMaxTurns          bool   `json:"supports_max_turns"`
	SupportsSystemPrompt      bool   `json:"supports_system_prompt"`
	ProviderProtocol          string `json:"provider_protocol"`
	SupportsFastMode          bool   `json:"supports_fast_mode"`
	SupportsCustomHTTPHeaders bool   `json:"supports_custom_http_headers"`
}
type InstallMetadata struct {
	RequiredBinary string `json:"required_binary,omitempty"`
	InstallHint    string `json:"install_hint,omitempty"`
}
type Descriptor struct {
	Runtime         Runtime
	Inspector       Inspector
	PermissionPlans []PermissionPlan
	Config          ConfigSurface
	Install         InstallMetadata
}
type Description struct {
	ID              string                 `json:"id"`
	Execution       Capabilities           `json:"execution"`
	Inspection      InspectionCapabilities `json:"inspection"`
	PermissionPlans []PermissionPlan       `json:"permission_plans"`
	Config          ConfigSurface          `json:"config"`
	Install         InstallMetadata        `json:"install"`
}

func DescribeRuntime(runtime Runtime) Descriptor {
	return Descriptor{Runtime: runtime, Config: ConfigSurface{ProviderProtocol: "native"}}
}

func (d Descriptor) Clone() Descriptor {
	d.PermissionPlans = append([]PermissionPlan(nil), d.PermissionPlans...)
	return d
}
func (d Descriptor) CanSatisfy(profile string) bool {
	for _, plan := range d.PermissionPlans {
		if plan.ProfileID == profile && (plan.Relation == PermissionEquivalent || plan.Relation == PermissionStricter) {
			return true
		}
	}
	return false
}
func (d Descriptor) Description(id string) Description {
	caps := InspectionCapabilities{Level: InspectionUnsupported}
	if !nilCapability(d.Inspector) {
		caps = d.Inspector.InspectionCapabilities()
	}
	var execution Capabilities
	if !nilCapability(d.Runtime) {
		execution = d.Runtime.Capabilities()
	}
	return Description{ID: id, Execution: execution, Inspection: caps, PermissionPlans: append([]PermissionPlan(nil), d.PermissionPlans...), Config: d.Config, Install: d.Install}
}
func nilCapability(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return v.IsNil()
	default:
		return false
	}
}
func (d Descriptor) Validate() error {
	bounded := func(s string, max int) bool {
		return len(s) <= max && utf8.ValidString(s) && !strings.ContainsAny(s, "\x00\r\n")
	}
	if nilCapability(d.Runtime) || d.Config.ProviderProtocol == "" || !bounded(d.Config.ProviderProtocol, 128) || !bounded(d.Install.RequiredBinary, 128) || !bounded(d.Install.InstallHint, 1024) || len(d.PermissionPlans) > 32 {
		return errors.New("bounded engine runtime/descriptor required")
	}
	seen := make(map[string]bool)
	for _, plan := range d.PermissionPlans {
		if plan.ProfileID == "" || !bounded(plan.ProfileID, 128) || !bounded(plan.Summary, 1024) || seen[plan.ProfileID] || (plan.Relation != PermissionEquivalent && plan.Relation != PermissionStricter) {
			return errors.New("invalid or duplicate engine permission plan")
		}
		seen[plan.ProfileID] = true
	}
	if d.Inspector != nil {
		if nilCapability(d.Inspector) {
			return errors.New("typed nil engine Inspector")
		}
		c := d.Inspector.InspectionCapabilities()
		if c.Level != InspectionFull && c.Level != InspectionSummary && c.Level != InspectionUnsupported {
			return errors.New("unknown engine inspection level")
		}
		if c.Level == InspectionUnsupported && (c.Historical || c.Live || c.RawRecords || c.RecursiveChildren || c.ExportJSONL) {
			return errors.New("unsupported inspection cannot claim capabilities")
		}
	}
	return nil
}
