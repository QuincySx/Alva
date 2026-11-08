// INPUT: Product TranscriptService pages of chart replies, the actual NativePage renderer and headless MyGo frames
// OUTPUT: Every chart type drawn, streaming placeholder, invalid and unclosed fallbacks without reveal flashes, lone-point dots, stacked gaps, pie folding/keys, data toggle, dark palette, scroll-retained view state, bounded cache estimate and opt-in 5×2000-point line/area frame gates
// POS: [L7 Hub UI tests] Chart data blocks in the Message Stream without a model, process or OS window
//go:build desktop

package hubdesktop

import (
	"fmt"
	"image/color"
	"image/png"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

var chartFixtures = map[string]string{
	"bar":     `{"type":"bar","title":"月销售额","x":["1月","2月","3月","4月"],"series":[{"name":"A","data":[12,18,9,15]},{"name":"B","data":[7,null,11,4]}],"unit":"万元"}`,
	"line":    `{"type":"line","title":"日活","x":["周一","周二","周三","周四","周五"],"series":[{"name":"今年","data":[3,5,null,6,8]},{"name":"去年","data":[2,3,4,4,5]}]}`,
	"area":    `{"type":"area","title":"流量构成","x":[2019,2020,2021,2022],"series":[{"name":"搜索","data":[4,6,7,9]},{"name":"直接","data":[2,3,3,4]}],"stacked":true}`,
	"pie":     `{"type":"pie","title":"平台分布","x":["macOS","Windows","Linux"],"series":[{"name":"用户","data":[52,33,15]}],"unit":"%"}`,
	"scatter": `{"type":"scatter","title":"身高与体重","series":[{"name":"甲组","data":[[160,52],[170,63],[182,77]]},{"name":"乙组","data":[[155,48],[175,70]]}]}`,
}

func chartReply(json string) string { return "结果如下：\n\n```chart\n" + json + "\n```\n" }

func chartHistory(session string, reply string, streaming bool) []hub.HistoryEntry {
	kind := engine.Message
	if streaming {
		kind = engine.MessageDelta
	}
	return []hub.HistoryEntry{
		{Sequence: 1, SessionID: session, TurnID: "turn", Input: "画一张图"},
		{Sequence: 2, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-chart"}},
		{Sequence: 3, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: kind, ID: "reply", Text: reply}},
	}
}

func chartPage(t *testing.T, entries func(session string) []hub.HistoryEntry, width, height int) (*NativePage, *ui.Tester) {
	t.Helper()
	service, requests := fixture(t)
	session, err := service.Create(t.Context(), "Charts", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.ctx = t.Context()
	p.selected = session.ID
	if p.detail, err = service.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	replaceTranscript(t, p, entries(session.ID)...)
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), width, height)
	t.Cleanup(func() {
		if len(requests) != 0 || service.Runtime.HasActive() {
			t.Error("chart presentation started execution")
		}
	})
	return p, tester
}

func saveChartScreenshot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_CHART_SCREENSHOT_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

const chartLabel = "attempt/reply#2"

func TestWorkspaceChartBlocksDrawEveryType(t *testing.T) {
	for _, name := range []string{"bar", "line", "area", "pie", "scatter"} {
		t.Run(name, func(t *testing.T) {
			p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(chartFixtures[name]), false) }, 1240, 900)
			card, ok := tester.Find("Chart block " + chartLabel)
			if !ok {
				t.Fatal("chart card missing", tester.Texts())
			}
			plot, ok := tester.Find("Chart plot " + chartLabel)
			if !ok || plot.H < 150 || plot.W < 150 || plot.Y < card.Y || plot.Y+plot.H > card.Y+card.H {
				t.Fatal("plot missing or outside its card", plot, card)
			}
			if tester.HasText("图表数据无效") || tester.HasText("正在生成图表…") || !tester.HasText("查看数据") {
				t.Fatal("valid chart fell back or lacks the data toggle", tester.Texts())
			}
			_, legend := tester.Find("Chart legend")
			if !legend {
				t.Fatal("multi-series or pie chart lacks a legend")
			}
			for _, text := range map[string][]string{"bar": {"月销售额", "单位：万元", "A", "B"}, "line": {"日活", "今年", "去年"}, "area": {"流量构成", "2020"}, "pie": {"平台分布", "macOS：52 %（52.0%）"}, "scatter": {"身高与体重", "甲组"}}[name] {
				if !tester.HasText(text) {
					t.Fatal("chart text missing", text, tester.Texts())
				}
			}
			saveChartScreenshot(t, tester, "chart-"+name+"-light.png")
			tester.SetDark(true)
			tester.Frame()
			saveChartScreenshot(t, tester, "chart-"+name+"-dark.png")
			if p.charts.parses != 1 {
				t.Fatal("one block text parsed more than once across frames", p.charts.parses)
			}
		})
	}
}

func chartPixel(t *testing.T, tester *ui.Tester, x, y float32) color.RGBA {
	t.Helper()
	img := tester.Image()
	scale := float32(img.Bounds().Dx()) / 1240
	return img.RGBAAt(int(x*scale), int(y*scale))
}

func nearColor(got color.RGBA, want ui.Color) bool {
	d := func(a, b uint8) int { return int(math.Abs(float64(int(a) - int(b)))) }
	return d(got.R, want.R) < 12 && d(got.G, want.G) < 12 && d(got.B, want.B) < 12
}

func TestWorkspaceChartBlocksFollowDarkTheme(t *testing.T) {
	single := `{"type":"bar","x":["only"],"series":[{"data":[10]}]}`
	_, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(single), false) }, 1240, 900)
	plot, ok := tester.Find("Chart plot " + chartLabel)
	if !ok {
		t.Fatal("plot missing")
	}
	x, y := plot.X+plot.W/2, plot.Y+plot.H*0.6
	if got := chartPixel(t, tester, x, y); !nearColor(got, chartPaletteLight[0]) {
		t.Fatal("light bar does not use the light palette", got)
	}
	tester.SetDark(true)
	tester.Frame()
	if got := chartPixel(t, tester, x, y); !nearColor(got, chartPaletteDark[0]) {
		t.Fatal("dark bar does not use the dark palette", got)
	}
	if muted, ok := tester.Find("Chart plot " + chartLabel); !ok || muted != plot {
		t.Fatal("theme change moved the plot", muted, plot)
	}
}

func TestWorkspaceChartBlockStreamingShowsPlaceholderNotErrors(t *testing.T) {
	half := "结果如下：\n\n```chart\n{\"type\":\"bar\",\"x\":[\"a\""
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, half, true) }, 1240, 900)
	tester.Advance(2 * time.Second) // let the streamed reveal reach the end
	tester.Frame()
	if _, ok := tester.Find("Chart pending " + chartLabel); !ok || !tester.HasText("正在生成图表…") || tester.HasText("图表数据无效") {
		t.Fatal("open streaming block did not show the placeholder", tester.Texts())
	}
	saveChartScreenshot(t, tester, "chart-streaming-placeholder.png")
	pending, _ := tester.Find("Chart pending " + chartLabel)
	// Closed but still invalid while the reply streams: still no error.
	appendTranscript(t, p, hub.HistoryEntry{Sequence: 4, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: ",\"b\"]}\n```\n还在写"}})
	p.applyDetail()
	tester.Advance(2 * time.Second)
	tester.Frame()
	if tester.HasText("图表数据无效") || !tester.HasText("正在生成图表…") {
		t.Fatal("streaming reply surfaced a chart error early", tester.Texts())
	}
	// The finished, still invalid reply falls back to the original text.
	if row := p.transcriptRows[len(p.transcriptRows)-1]; row.Status != "running" {
		t.Fatal("fixture reply is not streaming", row.Status)
	}
	replaceTranscript(t, p, chartHistory(p.selected, half+",\"b\"]}\n```\n", false)...)
	p.applyDetail()
	tester.Advance(2 * time.Second)
	tester.Frame()
	if !tester.HasText("图表数据无效：缺少 series") || tester.HasText("正在生成图表…") || !tester.HasText("chart") {
		t.Fatal("completed invalid chart did not fall back to its code with a reason", tester.Texts())
	}
	saveChartScreenshot(t, tester, "chart-invalid-fallback.png")
	if card, ok := tester.Find("Chart pending " + chartLabel); ok || pending.H < chartPlotHeight {
		t.Fatal("placeholder survived completion or lacked the fixed plot height", card, pending)
	}
}

func TestWorkspaceChartBlockStreamingBecomesChartWhenValid(t *testing.T) {
	text := "结果如下：\n\n```chart\n" + chartFixtures["line"] + "\n```\n"
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, text, true) }, 1240, 900)
	tester.Advance(2 * time.Second)
	tester.Frame()
	if _, ok := tester.Find("Chart plot " + chartLabel); !ok {
		t.Fatal("a closed valid block in a streaming reply did not draw", tester.Texts())
	}
	streaming, _ := tester.Find("Chart block " + chartLabel)
	replaceTranscript(t, p, chartHistory(p.selected, text, false)...)
	p.applyDetail()
	tester.Frame()
	finished, ok := tester.Find("Chart block " + chartLabel)
	if !ok || finished.H != streaming.H {
		t.Fatal("completion changed the chart's fixed height", streaming, finished)
	}
}

func TestWorkspaceChartBlockToggleShowsDataAndSurvivesScroll(t *testing.T) {
	filler := strings.Repeat("后续段落，用来把图表推出可视范围。", 6)
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry {
		entries := chartHistory(s, chartReply(chartFixtures["bar"]), false)
		for turn := 0; turn < 30; turn++ {
			attempt := fmt.Sprintf("later-%d", turn)
			entries = append(entries,
				hub.HistoryEntry{Sequence: uint64(10 + 2*turn), SessionID: s, TurnID: attempt, Input: fmt.Sprintf("问题 %d", turn)},
				hub.HistoryEntry{Sequence: uint64(11 + 2*turn), SessionID: s, AttemptID: attempt, Event: engine.Event{Kind: engine.Message, ID: "reply", Text: filler}})
		}
		return entries
	}, 1240, 900)
	list, ok := tester.Find("Message history")
	if !ok {
		t.Fatal("message list missing")
	}
	// Opening follows the end; scroll up until the whole first turn, from the
	// question above the chart to its toggle below, is unclipped.
	inside := func() bool {
		question, ok := tester.Find("User message turn/input")
		toggle, shown := tester.Find("Chart data toggle " + chartLabel)
		return ok && shown && question.H > 0 && question.Y > list.Y && toggle.H > 0 && toggle.Y+toggle.H < list.Y+list.H
	}
	for range 200 {
		if inside() {
			break
		}
		tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, -120)
		tester.Frame()
	}
	if err := tester.Click("Chart data toggle " + chartLabel); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if _, ok := tester.Find("Chart plot " + chartLabel); ok || !tester.HasText("查看图表") || !tester.HasText(chartFixtures["bar"]) {
		t.Fatal("toggle did not show the original JSON", tester.Texts())
	}
	saveChartScreenshot(t, tester, "chart-view-data.png")
	for range 200 {
		if _, ok := tester.Find("Chart block " + chartLabel); !ok {
			break
		}
		tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, 240)
		tester.Frame()
	}
	if _, ok := tester.Find("Chart block " + chartLabel); ok {
		t.Fatal("chart never left the screen")
	}
	for range 200 {
		if inside() {
			break
		}
		tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, -120)
		tester.Frame()
	}
	if _, ok := tester.Find("Chart plot " + chartLabel); ok || !tester.HasText("查看图表") {
		t.Fatal("data view was lost after scrolling away and back", tester.Texts())
	}
	if err := tester.Click("Chart data toggle " + chartLabel); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if _, ok := tester.Find("Chart plot " + chartLabel); !ok || !tester.HasText("查看数据") {
		t.Fatal("toggle did not return to the chart")
	}
	if p.charts.parses != 1 {
		t.Fatal("scrolling re-parsed the same block text", p.charts.parses)
	}
}

func TestWorkspaceChartBlockKeyboardSelectionReadsValues(t *testing.T) {
	_, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(chartFixtures["bar"]), false) }, 1240, 900)
	plot, _ := tester.Find("Chart plot " + chartLabel)
	tester.Move(plot.X+plot.W*0.15, plot.Y+plot.H/2)
	tester.Frame()
	if !tester.HasText("A：12 万元") || !tester.HasText("B：7 万元") {
		t.Fatal("hover did not read the first category", tester.Texts())
	}
	saveChartScreenshot(t, tester, "chart-bar-hover.png")
	// Clicking focuses the plot (one Tab stop); arrows then move the reading.
	tester.ClickAt(plot.X+plot.W*0.15, plot.Y+plot.H/2)
	tester.Frame()
	tester.Key(0, ui.KeyRight)
	tester.Frame()
	if !tester.HasText("B：无数据") || !tester.HasText("A：18 万元") {
		t.Fatal("keyboard did not move the reading to the next category", tester.Texts())
	}
}

func TestWorkspaceChartCacheAndTraceStayBounded(t *testing.T) {
	p, _ := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, "plain", false) }, 800, 600)
	for i := range 3 * chartCacheEntries {
		p.charts.entry(fmt.Sprintf(`{"type":"bar","x":["a"],"series":[{"data":[%d]}]}`, i+1))
	}
	if len(p.charts.entries) > chartCacheEntries || p.charts.cached > chartCacheBytes {
		t.Fatal("parse cache is unbounded", len(p.charts.entries), p.charts.cached)
	}
	for i := range 2 * chartViewEntries {
		p.charts.view(chartViewKey{"s", fmt.Sprint(i), 1})
	}
	if len(p.charts.views) > chartViewEntries || len(p.charts.viewOrder) > chartViewEntries {
		t.Fatal("view state is unbounded", len(p.charts.views))
	}
	if p.trace.markdown == nil || p.trace.markdown.Code["chart"] != nil {
		t.Fatal("trace details must keep chart blocks as original text")
	}
}

// chartColorPixels counts plot pixels near a palette color.
func chartColorPixels(t *testing.T, tester *ui.Tester, want ui.Color) int {
	t.Helper()
	plot, ok := tester.Find("Chart plot " + chartLabel)
	if !ok {
		t.Fatal("plot missing", tester.Texts())
	}
	img := tester.Image()
	scale := float32(img.Bounds().Dx()) / 1240
	n := 0
	for y := int(plot.Y * scale); y < int((plot.Y+plot.H)*scale); y++ {
		for x := int(plot.X * scale); x < int((plot.X+plot.W)*scale); x++ {
			if nearColor(img.RGBAAt(x, y), want) {
				n++
			}
		}
	}
	return n
}

func chartSelected(p *NativePage) int {
	for _, v := range p.charts.views {
		return v.selected
	}
	return -2
}

// A reply that ended inside a chart block (stopped, failed, or cut at the
// 60 KiB final-message limit) never gets its closing fence.
func TestWorkspaceChartBlockUnclosedInFinishedReplyShowsSource(t *testing.T) {
	for name, text := range map[string]string{
		"valid json":   "结果如下：\n\n```chart\n" + chartFixtures["bar"] + "\n",
		"cut mid json": "结果如下：\n\n```chart\n" + chartFixtures["bar"][:40],
		"bare fence":   "结果如下：\n\n```chart\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, text, false) }, 1240, 900)
			tester.Advance(2 * time.Second)
			tester.Frame()
			if tester.HasText("正在生成图表…") || !tester.HasText("图表数据无效：代码块未闭合") {
				t.Fatal("finished reply kept an unclosed block pending", tester.Texts())
			}
			if _, ok := tester.Find("Chart plot " + chartLabel); ok {
				t.Fatal("unclosed block drew a chart")
			}
		})
	}
	// While the reply still streams, the same text is only pending.
	_, tester := chartPage(t, func(s string) []hub.HistoryEntry {
		return chartHistory(s, "结果如下：\n\n```chart\n"+chartFixtures["bar"]+"\n", true)
	}, 1240, 900)
	tester.Advance(2 * time.Second)
	tester.Frame()
	if !tester.HasText("正在生成图表…") || tester.HasText("代码块未闭合") {
		t.Fatal("streaming open block reported an error", tester.Texts())
	}
}

// Right after completion the reveal animation still shows a prefix; the block
// looks open there, but the full text closes it, so no error may flash.
func TestWorkspaceChartBlockCompletionRevealDoesNotFlashUnclosed(t *testing.T) {
	head := "结果如下：\n\n```chart\n"
	full := head + chartFixtures["line"] + "\n```\n\n后面还有一段说明文字。"
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, head, true) }, 1240, 900)
	tester.Frame()
	replaceTranscript(t, p, chartHistory(p.selected, full, false)...)
	p.applyDetail()
	sawReveal := false
	for range 40 {
		tester.Advance(10 * time.Millisecond)
		tester.Frame()
		if tester.HasText("代码块未闭合") || tester.HasText("图表数据无效") {
			t.Fatal("the reveal of a completed, closed block flashed an error", tester.Texts())
		}
		if tester.HasText("正在生成图表…") {
			sawReveal = true
		}
	}
	tester.Advance(2 * time.Second)
	tester.Frame()
	if !sawReveal {
		t.Fatal("fixture never observed the post-completion reveal; the check would be vacuous")
	}
	if _, ok := tester.Find("Chart plot " + chartLabel); !ok {
		t.Fatal("completed block never drew after the reveal", tester.Texts())
	}
}

func TestWorkspaceChartLonePointsDrawDots(t *testing.T) {
	for name, data := range map[string]string{
		"single":    `{"type":"line","x":["a"],"series":[{"data":[5]}]}`,
		"numeric x": `{"type":"line","x":[2020],"series":[{"data":[5]}]}`,
		"isolated":  `{"type":"line","x":["a","b","c","d","e"],"series":[{"data":[1,null,2,null,3]}]}`,
		"area":      `{"type":"area","x":["a"],"series":[{"data":[5]}]}`,
		"stacked":   `{"type":"area","stacked":true,"x":["a","b","c"],"series":[{"data":[1,null,2]},{"data":[1,1,1]}]}`,
	} {
		t.Run(name, func(t *testing.T) {
			_, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(data), false) }, 1240, 900)
			if n := chartColorPixels(t, tester, chartPaletteLight[0]); n < 10 {
				t.Fatal("a run of one point drew nothing", n)
			}
		})
	}
}

func TestWorkspaceChartStackedAreaNullIsAGap(t *testing.T) {
	value, err := product.ParseChart(`{"type":"area","stacked":true,"x":["a","b","c"],"series":[{"data":[1,null,2]},{"data":[1,1,1]}]}`)
	if err != nil {
		t.Fatal(err)
	}
	p := newChartPlot(value)
	if !math.IsNaN(float64(p.ys[0][1])) || !math.IsNaN(float64(p.lo[0][1])) {
		t.Fatal("stacked null was drawn as zero instead of a gap", p.ys[0], p.lo[0])
	}
	for i := range 3 {
		if math.IsNaN(float64(p.ys[1][i])) || p.lo[1][i] == p.ys[1][i] {
			t.Fatal("series above a gap lost its band", i, p.lo[1], p.ys[1])
		}
	}
	// It takes no room: the upper band sits on zero where the null is.
	if p.lo[1][1] != p.y.At(0) || p.lo[1][0] == p.y.At(0) {
		t.Fatal("null took room in the stack", p.lo[1])
	}
	if len(p.dots[0]) != 2 {
		t.Fatal("points beside the gap are lone runs and need dots", p.dots[0])
	}
}

func TestWorkspaceChartPieReadsFoldedSlices(t *testing.T) {
	data := `{"type":"pie","x":["a","b","c","d","e","f","g","HHH","i","j"],"series":[{"data":[10,9,8,7,6,5,4,33,2,1]}]}`
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(data), false) }, 1240, 900)
	for selected, text := range map[int]string{7: "HHH：33（38.8%）", 8: "i：2（2.4%）"} {
		for _, v := range p.charts.views {
			v.selected = selected
		}
		tester.Frame()
		if !tester.HasText(text) || !tester.HasText("其他 3 项：36（42.4%）") {
			t.Fatal("folded slice reading missing", selected, tester.Texts())
		}
	}
}

func TestWorkspaceChartPieKeyboardWorksWhileHoveredAndLeavingClears(t *testing.T) {
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(chartFixtures["pie"]), false) }, 1240, 900)
	ring, _ := tester.Find("Chart plot " + chartLabel)
	tester.ClickAt(ring.X+ring.W*0.6, ring.Y+ring.H*0.3) // the first slice, macOS
	tester.Frame()
	if chartSelected(p) != 0 {
		t.Fatal("hover did not choose the slice under the pointer", chartSelected(p))
	}
	tester.Key(0, ui.KeyRight)
	tester.Frame()
	tester.Frame()
	if chartSelected(p) != 1 || !tester.HasText("Windows：33 %（33.0%）") {
		t.Fatal("arrow key choice was overridden by the resting pointer", chartSelected(p))
	}
	tester.Move(ring.X+ring.W*0.6, ring.Y+ring.H*0.31)
	tester.Frame()
	if chartSelected(p) != 0 {
		t.Fatal("moving the pointer did not choose again", chartSelected(p))
	}
	tester.Move(5, 5)
	tester.Frame()
	if chartSelected(p) != -1 {
		t.Fatal("leaving the pie kept a choice", chartSelected(p))
	}
}

func TestWorkspaceChartAreaReductionAndCacheEstimate(t *testing.T) {
	xs, hi := make([]float32, 2000), make([]float32, 2000)
	for i := range xs {
		xs[i], hi[i] = float32(i)/1999, float32(i%97)/97
	}
	hi[1000] = float32(math.NaN())
	idx := chartColumns(nil, xs, nil, hi, 300)
	breaks := 0
	for _, i := range idx {
		if i < 0 {
			breaks++
		}
	}
	if len(idx) > 4*301+2 || breaks != 1 || idx[0] != 0 || idx[len(idx)-1] != 1999 {
		t.Fatal("area reduction is not bounded by pixel columns or lost run ends/breaks", len(idx), breaks)
	}
	// The estimate covers what the entry keeps after drawing.
	points, a, b := make([]string, 2000), make([]string, 2000), make([]string, 2000)
	for i := range points {
		points[i], a[i], b[i] = fmt.Sprint(i), fmt.Sprint(i%97), fmt.Sprint((i*7)%89-40)
	}
	labels := make([]string, 2000)
	for i := range labels {
		labels[i] = fmt.Sprintf("%q", fmt.Sprintf("d%d", i))
	}
	data := `{"type":"area","x":[` + strings.Join(labels, ",") + `],"series":[{"data":[` + strings.Join(a, ",") + `]},{"data":[` + strings.Join(b, ",") + `]}]}`
	p, tester := chartPage(t, func(s string) []hub.HistoryEntry { return chartHistory(s, chartReply(data), false) }, 1240, 900)
	tester.Frame()
	e := p.charts.entries[data+"\n"]
	if e == nil || e.err != nil {
		t.Fatal("bench-sized area chart not cached", e)
	}
	kept := len(data+"\n") + len(e.chart.Labels)*16 + cap(e.plot.xs)*4 + cap(e.plot.bx)*4 + cap(e.plot.by)*4 + cap(e.plot.idx)*8
	for _, label := range e.chart.Labels {
		kept += len(label)
	}
	for s, series := range e.chart.Series {
		kept += cap(series.Values)*8 + cap(e.plot.ys[s])*4
	}
	if e.bytes < kept {
		t.Fatal("cache byte estimate is below what the entry keeps", e.bytes, kept)
	}
}

// ALVA_UI_BENCH=1 scrolls transcripts where five 2000-point line charts, then
// five two-series 2000-point area charts (one series dipping below zero), are
// on screen at once; it measures headless build/layout/paint CPU only.
func TestWorkspaceChartScrollFrameBudget(t *testing.T) {
	if os.Getenv("ALVA_UI_BENCH") != "1" {
		t.Skip("set ALVA_UI_BENCH=1 to run the chart frame-time gate")
	}
	xs, line, a, b := make([]string, 2000), make([]string, 2000), make([]string, 2000), make([]string, 2000)
	for i := range xs {
		xs[i] = fmt.Sprint(i)
		line[i] = fmt.Sprintf("%.3f", 50+30*math.Sin(float64(i)/40)+10*math.Sin(float64(i)/7))
		a[i], b[i] = fmt.Sprint(i%97), fmt.Sprint((i*7)%89-40)
	}
	for kind, data := range map[string]string{
		"line": `{"type":"line","title":"2000 点","x":[` + strings.Join(xs, ",") + `],"series":[{"name":"s","data":[` + strings.Join(line, ",") + `]}]}`,
		"area": `{"type":"area","title":"2000 点","x":[` + strings.Join(xs, ",") + `],"series":[{"name":"a","data":[` + strings.Join(a, ",") + `]},{"name":"b","data":[` + strings.Join(b, ",") + `]}]}`,
	} {
		t.Run(kind, func(t *testing.T) {
			if len(data) > product.ChartMaxBytes {
				t.Fatal("bench chart exceeds the format limit", len(data))
			}
			p, tester := chartPage(t, func(s string) []hub.HistoryEntry {
				var entries []hub.HistoryEntry
				for turn := 0; turn < 60; turn++ {
					attempt := fmt.Sprintf("attempt-%d", turn)
					// Distinct titles keep five distinct cache entries per screen.
					body := strings.Replace(data, "2000 点", fmt.Sprintf("2000 点 #%d", turn), 1)
					entries = append(entries,
						hub.HistoryEntry{Sequence: uint64(2*turn + 1), SessionID: s, TurnID: attempt, AttemptID: attempt, Input: "图"},
						hub.HistoryEntry{Sequence: uint64(2*turn + 2), SessionID: s, AttemptID: attempt, Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "```chart\n" + body + "\n```\n"}})
				}
				return entries
			}, 1240, 1900)
			list, ok := tester.Find("Message history")
			if !ok {
				t.Fatal("message list missing")
			}
			visible := func() int {
				n := 0
				for turn := 0; turn < 60; turn++ {
					if _, ok := tester.Find(fmt.Sprintf("Chart plot attempt-%d/reply#1", turn)); ok {
						n++
					}
				}
				return n
			}
			if n := visible(); n < 5 {
				t.Fatal("bench screen does not hold five charts", n)
			}
			var frames []time.Duration
			minimum := 99
			for _, delta := range []float32{-120, 120, -120} {
				for range 150 {
					start := time.Now()
					tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
					tester.Frame()
					frames = append(frames, time.Since(start))
				}
				minimum = min(minimum, visible())
			}
			sort.Slice(frames, func(i, j int) bool { return frames[i] < frames[j] })
			p95 := frames[len(frames)*95/100]
			t.Logf("five 2000-point %s charts headless scroll: p50=%s p95=%s max=%s n=%d parses=%d visible>=%d", kind, frames[len(frames)/2], p95, frames[len(frames)-1], len(frames), p.charts.parses, minimum)
			if minimum < 5 {
				t.Fatal("scrolling left fewer than five charts on screen", minimum)
			}
			if p95 > 8*time.Millisecond {
				t.Fatal("chart scroll frame CPU p95 exceeds 8 ms", p95)
			}
		})
	}
}
