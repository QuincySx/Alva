// INPUT: TraceStep values of one window
// OUTPUT: TraceScale, TraceLane, TraceBar, TraceGroup and LayoutTrace normalized timeline geometry
// POS: [L7 Hub product] UI-independent timeline placement as fractions of the axis; no drawing, I/O or MyGo
package product

import (
	"strconv"
	"time"
)

// TraceScale chooses how the timeline spreads steps along its axis.
type TraceScale string

const (
	// TraceScaleDuration places steps by real time.
	TraceScaleDuration TraceScale = "duration"
	// TraceScaleTurn, TraceScaleCall and TraceScaleStep compress the axis into
	// groups by product Turn, by model call (the call and the tools it asked
	// for) or one per step.
	TraceScaleTurn TraceScale = "turn"
	TraceScaleCall TraceScale = "call"
	TraceScaleStep TraceScale = "step"
)

// TraceLane is a timeline swim lane.
type TraceLane int

const (
	TraceLaneInput TraceLane = iota
	TraceLaneModel
	TraceLaneTool
)

// LaneOf places input on the input lane, model calls on the model lane and
// tools and child Agents on the tool lane.
func LaneOf(kind TraceStepKind) TraceLane {
	switch kind {
	case TraceStepInput:
		return TraceLaneInput
	case TraceStepModel:
		return TraceLaneModel
	}
	return TraceLaneTool
}

// TraceBar places one step: Step indexes the steps given to LayoutTrace and
// X0..X1 are fractions of the axis (equal for a point).
type TraceBar struct {
	Step   int
	Lane   TraceLane
	X0, X1 float32
}

// TraceGroup is one compressed group's span and label.
type TraceGroup struct {
	Label  string
	X0, X1 float32
}

// TraceLayout is a window's timeline: bars, group spans for compressed
// scales, and the real-time extent for the duration scale.
type TraceLayout struct {
	Bars     []TraceBar
	Groups   []TraceGroup
	From, To time.Time
}

// traceTimes is a step's span with an open end closed at the window's latest
// known instant.
func traceTimes(step TraceStep, latest time.Time) (time.Time, time.Time) {
	start, end := step.Start, step.End
	if step.Open || end.IsZero() {
		end = latest
	}
	if end.Before(start) {
		end = start
	}
	return start, end
}

func latestTime(steps []TraceStep) time.Time {
	var latest time.Time
	for _, step := range steps {
		for _, at := range []time.Time{step.Start, step.End} {
			if at.After(latest) {
				latest = at
			}
		}
		for _, record := range step.Records {
			if record.Time.After(latest) {
				latest = record.Time
			}
		}
	}
	return latest
}

func fraction(at, from time.Time, span time.Duration) float32 {
	if span <= 0 {
		return 0
	}
	return float32(float64(at.Sub(from)) / float64(span))
}

// groupKeys assigns each step a compressed group.
func groupKeys(steps []TraceStep, scale TraceScale) ([]int, []string) {
	keys := make([]int, len(steps))
	var labels []string
	group := -1
	lastTurn, callOpen := uint64(1<<63), false
	for i, step := range steps {
		switch scale {
		case TraceScaleTurn:
			if group < 0 || step.Turn != lastTurn || step.Turn == 0 && step.Kind == TraceStepInput {
				group++
				label := "第 ? 轮"
				if step.Turn != 0 {
					label = "第 " + strconv.FormatUint(step.Turn, 10) + " 轮"
				}
				labels = append(labels, label)
			}
			lastTurn = step.Turn
		case TraceScaleCall:
			// A model call opens a group its tools and child Agents join.
			if group < 0 || step.Kind == TraceStepModel || step.Kind == TraceStepInput || !callOpen {
				group++
				label := "输入"
				switch step.Kind {
				case TraceStepModel:
					label = "调用"
				case TraceStepTool, TraceStepChild:
					label = "调用 …"
				}
				labels = append(labels, label)
			}
			callOpen = step.Kind != TraceStepInput
		default:
			group++
			labels = append(labels, "#"+strconv.Itoa(group+1))
		}
		keys[i] = group
	}
	return keys, labels
}

// LayoutTrace places a window's steps on a normalized axis. Duration uses real
// time over the window's extent. Compressed scales give each group a width in
// proportion to its step count with a small gap between groups; inside a group
// steps keep their real relative times, falling back to equal slots in order
// when the group has no measurable duration.
func LayoutTrace(steps []TraceStep, scale TraceScale) TraceLayout {
	var layout TraceLayout
	if len(steps) == 0 {
		return layout
	}
	latest := latestTime(steps)
	from := steps[0].Start
	for _, step := range steps {
		if !step.Start.IsZero() && (from.IsZero() || step.Start.Before(from)) {
			from = step.Start
		}
	}
	layout.From, layout.To = from, latest
	if scale == TraceScaleDuration {
		span := latest.Sub(from)
		for i, step := range steps {
			start, end := traceTimes(step, latest)
			layout.Bars = append(layout.Bars, TraceBar{Step: i, Lane: LaneOf(step.Kind), X0: fraction(start, from, span), X1: fraction(end, from, span)})
		}
		return layout
	}
	keys, labels := groupKeys(steps, scale)
	count := len(labels)
	weights := make([]int, count)
	for _, key := range keys {
		weights[key]++
	}
	gapTotal := float32(0)
	if count > 1 {
		gapTotal = min(0.25, 0.012*float32(count-1))
	}
	gap := float32(0)
	if count > 1 {
		gap = gapTotal / float32(count-1)
	}
	usable := 1 - gapTotal
	starts := make([]float32, count)
	widths := make([]float32, count)
	cursor := float32(0)
	for g := range count {
		starts[g] = cursor
		widths[g] = usable * float32(weights[g]) / float32(len(steps))
		cursor += widths[g] + gap
		layout.Groups = append(layout.Groups, TraceGroup{Label: labels[g], X0: starts[g], X1: starts[g] + widths[g]})
	}
	for g := range count {
		var members []int
		var gFrom, gTo time.Time
		for i, key := range keys {
			if key != g {
				continue
			}
			members = append(members, i)
			start, end := traceTimes(steps[i], latest)
			if gFrom.IsZero() || start.Before(gFrom) {
				gFrom = start
			}
			if end.After(gTo) {
				gTo = end
			}
		}
		span := gTo.Sub(gFrom)
		for slot, i := range members {
			x0, x1 := float32(slot)/float32(len(members)), float32(slot+1)/float32(len(members))
			if span > 0 {
				start, end := traceTimes(steps[i], latest)
				x0, x1 = fraction(start, gFrom, span), fraction(end, gFrom, span)
			}
			layout.Bars = append(layout.Bars, TraceBar{Step: i, Lane: LaneOf(steps[i].Kind), X0: starts[g] + x0*widths[g], X1: starts[g] + x1*widths[g]})
		}
	}
	return layout
}
