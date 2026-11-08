// INPUT: product.ParseChart values, the MyGo ui/markdown code-block hook and ui/chart scales, marks, paths and selection
// OUTPUT: transcriptCharts: a bounded parse cache, per-block view state and the "chart" code-block renderer for Message Stream Markdown
// POS: [L7 Hub UI] Trusted native drawing of model-supplied chart data in the Message Stream only; the data is never executed
//go:build desktop

package hubdesktop

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/egoist/mygo/ui/markdown"
)

const (
	chartCacheEntries = 32
	chartCacheBytes   = 8 << 20
	chartViewEntries  = 256
	chartHeaderHeight = 20
	chartLegendHeight = 18
	chartPlotHeight   = 220
	chartFooterHeight = 28
	chartPieSize      = 200
)

// The categorical slots and their order are the validated reference palette
// (adjacent-pair CVD ΔE ≥ 8.4, normal-vision ΔE ≥ 19.3 on the Hub light and
// dark paper surfaces). Status colors stay reserved for state, never a series.
var (
	chartPaletteLight = [...]ui.Color{ui.Hex("#2a78d6"), ui.Hex("#eb6834"), ui.Hex("#1baf7a"), ui.Hex("#eda100"), ui.Hex("#e87ba4"), ui.Hex("#008300"), ui.Hex("#6250d6"), ui.Hex("#e34948")}
	chartPaletteDark  = [...]ui.Color{ui.Hex("#3987e5"), ui.Hex("#d95926"), ui.Hex("#199e70"), ui.Hex("#c98500"), ui.Hex("#d55181"), ui.Hex("#008300"), ui.Hex("#9085e9"), ui.Hex("#e66767")}
)

func chartColor(t *ui.Theme, i int) ui.Color {
	if t.Dark {
		return chartPaletteDark[i%len(chartPaletteDark)]
	}
	return chartPaletteLight[i%len(chartPaletteLight)]
}

// chartViewKey names one chart block within one message of one session; the
// block index is stable because message text only grows or is replaced.
type chartViewKey struct {
	session, row string
	block        int
}

// chartView survives virtual-list recycling: a block scrolled away and back
// keeps its data/chart choice and keyboard selection.
type chartView struct {
	raw      bool
	selected int
}

type chartEntry struct {
	chart product.Chart
	err   error
	plot  *chartPlot
	used  uint64
	bytes int
}

// transcriptCharts belongs to one page and is used only on the UI thread.
type transcriptCharts struct {
	entries   map[string]*chartEntry
	cached    int
	clock     uint64
	parses    int
	views     map[chartViewKey]*chartView
	viewOrder []chartViewKey
	// The message whose Markdown is being built; set only around that call.
	// text is its complete text, ahead of what a reveal animation has shown.
	session, row, text string
	streaming          bool
}

func newTranscriptCharts() *transcriptCharts {
	return &transcriptCharts{entries: make(map[string]*chartEntry), views: make(map[chartViewKey]*chartView)}
}

// scope binds the chart hook to one message for the duration of build: the
// virtual list may build rows during layout, but each Markdown build is
// synchronous, so the hook always sees the message it is drawing.
func (t *transcriptCharts) scope(session, row, text string, streaming bool, build func()) {
	if t == nil {
		build()
		return
	}
	previousSession, previousRow, previousText, previousStreaming := t.session, t.row, t.text, t.streaming
	t.session, t.row, t.text, t.streaming = session, row, text, streaming
	defer func() {
		t.session, t.row, t.text, t.streaming = previousSession, previousRow, previousText, previousStreaming
	}()
	build()
}

// chartUnclosedAtEnd reports whether an open block's code already ends the
// message's complete text, so no closing fence can follow. Right after a reply
// completes, the reveal animation may still show a prefix: the block then looks
// open only because the rest has not been shown, and its code is not yet the
// end of the text. Whitespace is ignored so indented (list) fences compare too.
func chartUnclosedAtEnd(code, text string) bool {
	squeeze := func(s string) string {
		return strings.Map(func(r rune) rune {
			if unicode.IsSpace(r) {
				return -1
			}
			return r
		}, s)
	}
	code, text = squeeze(code), squeeze(text)
	if code == "" {
		// An opening fence with nothing after it.
		return strings.HasSuffix(text, "```chart") || strings.HasSuffix(text, "~~~chart")
	}
	return strings.HasSuffix(text, code)
}

// entry parses a block's text once and keeps the result, bounded by count and
// estimated bytes, evicting the least recently drawn.
func (t *transcriptCharts) entry(text string) *chartEntry {
	t.clock++
	if e, ok := t.entries[text]; ok {
		e.used = t.clock
		return e
	}
	t.parses++
	value, err := product.ParseChart(text)
	e := &chartEntry{chart: value, err: err, used: t.clock}
	if err == nil {
		e.plot = newChartPlot(value)
	}
	e.bytes = chartEntryBytes(text, e)
	for len(t.entries) > 0 && (len(t.entries) >= chartCacheEntries || t.cached+e.bytes > chartCacheBytes) {
		oldest, stamp := "", uint64(math.MaxUint64)
		for key, cached := range t.entries {
			if cached.used < stamp {
				oldest, stamp = key, cached.used
			}
		}
		t.cached -= t.entries[oldest].bytes
		delete(t.entries, oldest)
	}
	t.entries[text] = e
	t.cached += e.bytes
	return e
}

// chartEntryBytes estimates what one cache entry keeps: the key text, the
// parsed values and labels, the plot geometry, and the drawing buffers at the
// largest size a frame can grow them to: reduced points never exceed the
// points plus one break per run, the stroke path holds one 28-byte command per
// reduced point and an area's fill about twice that, with append slack.
func chartEntryBytes(text string, e *chartEntry) int {
	const header, entry = 24, 256
	n := len(text) + header + entry
	value := e.chart
	n += len(value.Title) + len(value.Unit) + len(value.XValues)*8
	for _, label := range value.Labels {
		n += header + len(label)
	}
	points := len(value.Labels)
	for _, series := range value.Series {
		n += header*3 + len(series.Name) + len(series.Values)*8 + len(series.Points)*16
		points = max(points, len(series.Values), len(series.Points))
	}
	p := e.plot
	if p == nil {
		return n
	}
	n += len(p.xs)*4 + len(p.slices)*24 + len(p.orderXs)*4 + len(p.order)*16 + (len(p.xTicks)+len(p.yTicks))*(header+32)
	for _, group := range [][][]float32{p.ys, p.lo, p.px, p.py} {
		for _, values := range group {
			n += header + len(values)*4
		}
	}
	for _, series := range p.stack {
		n += header + len(series)*16
	}
	for _, dots := range p.dots {
		n += header + len(dots)*8
	}
	reduced := points*3/2 + 2
	commands := reduced
	if value.Type == product.ChartArea {
		commands += 2*reduced + points/2
	}
	n += (reduced*(4+4+8) + commands*28) * 3 / 2 // bx, by, idx and paths
	return n
}

func (t *transcriptCharts) view(key chartViewKey) *chartView {
	if v, ok := t.views[key]; ok {
		return v
	}
	if len(t.viewOrder) >= chartViewEntries {
		delete(t.views, t.viewOrder[0])
		t.viewOrder = t.viewOrder[1:]
	}
	v := &chartView{selected: -1}
	t.views[key] = v
	t.viewOrder = append(t.viewOrder, key)
	return v
}

// render is the Markdown "chart" code-block hook.
func (t *transcriptCharts) render(c *ui.Context, block *markdown.CodeBlockView) {
	label := fmt.Sprintf("%s#%d", t.row, block.Index)
	if block.Open {
		if t.streaming || !chartUnclosedAtEnd(block.Code(), t.text) {
			chartPlaceholder(c, label)
			return
		}
		// A stopped, failed or truncated reply that ended inside the block:
		// no closing fence will ever come, so show what there is.
		chartInvalid(c, block, label, "代码块未闭合")
		return
	}
	e := t.entry(block.Code())
	if e.err != nil {
		if t.streaming {
			// The reply may still be correcting itself; no error until it ends.
			chartPlaceholder(c, label)
			return
		}
		chartInvalid(c, block, label, e.err.Error())
		return
	}
	v := t.view(chartViewKey{t.session, t.row, block.Index})
	th := c.Theme()
	ui.Column(c).Label("Chart block "+label).AlignSelf(ui.Stretch).FillWidth().Gap(8).Padding(12).Radius(12).
		Border(1, th.Border).Children(func() {
		chartHeader(c, e.chart)
		if v.raw {
			markdown.DefaultCodeBlock(c, block)
		} else {
			if len(e.chart.Series) > 1 && e.chart.Type != product.ChartPie {
				chartLegend(c, e.chart)
			}
			buildChartPlot(c, e, v, label)
		}
		ui.Row(c).Height(chartFooterHeight).AlignItems(ui.Center).Children(func() {
			text := "查看数据"
			if v.raw {
				text = "查看图表"
			}
			button := ui.ButtonBase(c.Key(fmt.Sprintf("chart-toggle-%d", block.Index))).Label("Chart data toggle "+label).
				Height(24).Padding(0, 8).Radius(6).Background(ui.Transparent).TextColor(th.TextMuted).
				FontSize(workspaceFontCaption)
			if button.Hovered() {
				button.Background(th.SurfaceHover)
			}
			button.OnClick(func() { v.raw = !v.raw }).Children(func() { ui.Text(c, text).SingleLine() })
		})
	})
}

// chartInvalid keeps the model's original text visible with one short reason.
func chartInvalid(c *ui.Context, block *markdown.CodeBlockView, label, reason string) {
	markdown.DefaultCodeBlock(c, block)
	ui.Row(c).Label("Chart invalid " + label).Children(func() {
		ui.Text(c, "图表数据无效："+reason).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	})
}

func chartPlaceholder(c *ui.Context, label string) {
	th := c.Theme()
	ui.Column(c).Label("Chart pending "+label).AlignSelf(ui.Stretch).FillWidth().Gap(8).Padding(12).Radius(12).
		Border(1, th.Border).Children(func() {
		ui.Text(c, "正在生成图表…").Height(chartHeaderHeight).FontSize(workspaceFontBody).TextColor(th.TextMuted).SingleLine()
		ui.Box(c).FillWidth().Height(chartPlotHeight).Radius(8).Background(workspacePalette(c).subtle)
		ui.Box(c).Height(chartFooterHeight)
	})
}

func chartHeader(c *ui.Context, value product.Chart) {
	th := c.Theme()
	title := value.Title
	if title == "" {
		title = map[product.ChartType]string{product.ChartBar: "柱状图", product.ChartLine: "折线图", product.ChartArea: "面积图", product.ChartPie: "饼图", product.ChartScatter: "散点图"}[value.Type]
	}
	ui.Row(c).Height(chartHeaderHeight).Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).TextColor(th.Text).SingleLine().MinWidth(0).Shrink(1)
		if value.Unit != "" {
			ui.Text(c, "单位："+value.Unit).FontSize(workspaceFontCaption).TextColor(th.TextMuted).SingleLine().Shrink(0)
		}
	})
}

func chartSeriesName(value product.Chart, i int) string {
	if name := value.Series[i].Name; name != "" {
		return name
	}
	return fmt.Sprintf("系列 %d", i+1)
}

func chartLegend(c *ui.Context, value product.Chart) {
	th := c.Theme()
	ui.Row(c).Label("Chart legend").Height(chartLegendHeight).Gap(12).AlignItems(ui.Center).Clip().Children(func() {
		for i := range value.Series {
			ui.Row(c).Gap(6).AlignItems(ui.Center).Shrink(0).MaxWidth(180).Children(func() {
				ui.Box(c).Size(10, 10).Radius(3).Background(chartColor(th, i)).Shrink(0)
				ui.Text(c, chartSeriesName(value, i)).FontSize(workspaceFontCaption).TextColor(th.TextMuted).SingleLine().MinWidth(0)
			})
		}
	})
}

// chartNumber formats a value for reading, without float noise.
func chartNumber(v float64, unit string) string {
	if math.IsNaN(v) {
		return "无数据"
	}
	text := strconv.FormatFloat(v, 'f', -1, 64)
	if a := math.Abs(v); len(text) > 12 || (a != 0 && (a < 1e-4 || a >= 1e12)) {
		text = strconv.FormatFloat(v, 'g', 6, 64)
	}
	if unit != "" {
		text += " " + unit
	}
	return text
}

func chartShort(label string, runes int) string {
	if utf8.RuneCountInString(label) <= runes {
		return label
	}
	out := []rune(label)[:runes-1]
	return string(out) + "…"
}

type chartPointRef struct{ series, point int }

// chartPlot is the data's geometry in plot fractions, computed once per
// parsed chart; drawing only maps fractions to the frame's rectangle.
type chartPlot struct {
	y              chart.Linear
	yTicks, xTicks []chart.Tick
	base           float32 // where zero is
	band           chart.Band
	stack          [][]chart.Segment
	xs             []float32   // categories or points along x, ascending
	ys             [][]float32 // per series; NaN is a gap
	lo             [][]float32 // stacked area lower edges; NaN with ys
	dots           [][]int     // per series, points with no defined neighbor
	slices         []chart.Slice
	total          float64
	px, py         [][]float32 // scatter
	order          []chartPointRef
	orderXs        []float32
	// Reused while drawing, so a warm frame allocates nothing per series.
	bx, by     []float32
	idx        []int
	path, fill ui.Path
}

func chartScale(lo, hi float64, zero bool) chart.Linear {
	if zero {
		lo, hi = min(lo, 0), max(hi, 0)
	}
	if lo == hi {
		lo, hi = lo-1, hi+1
	}
	return chart.NewLinear(lo, hi).Nice(4)
}

func chartFractions(scale chart.Linear, values []float64) []float32 {
	out := make([]float32, len(values))
	for i, v := range values {
		if math.IsNaN(v) {
			out[i] = float32(math.NaN())
		} else {
			out[i] = scale.At(v)
		}
	}
	return out
}

func chartCategoryTicks(labels []string, at func(i int) float32) []chart.Tick {
	every := max(1, (len(labels)+7)/8)
	var ticks []chart.Tick
	for i := 0; i < len(labels); i += every {
		ticks = append(ticks, chart.Tick{At: at(i), Value: float64(i), Label: chartShort(labels[i], 10)})
	}
	return ticks
}

func newChartPlot(value product.Chart) *chartPlot {
	p := &chartPlot{}
	switch value.Type {
	case product.ChartPie:
		p.slices = chart.Pie(value.Series[0].Values, 0.015)
		for _, v := range value.Series[0].Values {
			p.total += v
		}
		return p
	case product.ChartScatter:
		var xs, ys []float64
		for s, series := range value.Series {
			for i, point := range series.Points {
				xs, ys = append(xs, point.X), append(ys, point.Y)
				p.order = append(p.order, chartPointRef{s, i})
			}
		}
		xlo, xhi := chart.Extent(xs...)
		ylo, yhi := chart.Extent(ys...)
		x, y := chartScale(xlo, xhi, false), chartScale(ylo, yhi, false)
		p.y, p.yTicks, p.xTicks = y, y.Ticks(4), x.Ticks(5)
		for _, series := range value.Series {
			px, py := make([]float32, len(series.Points)), make([]float32, len(series.Points))
			for i, point := range series.Points {
				px[i], py[i] = x.At(point.X), y.At(point.Y)
			}
			p.px, p.py = append(p.px, px), append(p.py, py)
		}
		sort.SliceStable(p.order, func(i, j int) bool {
			a, b := p.order[i], p.order[j]
			return p.px[a.series][a.point] < p.px[b.series][b.point]
		})
		for _, ref := range p.order {
			p.orderXs = append(p.orderXs, p.px[ref.series][ref.point])
		}
		return p
	}
	values := make([][]float64, len(value.Series))
	for i, series := range value.Series {
		values[i] = series.Values
	}
	var lo, hi float64
	if value.Stacked {
		p.stack = chart.Stack(values, chart.StackStandard)
		var ends []float64
		for _, series := range p.stack {
			for _, segment := range series {
				ends = append(ends, segment.Lo, segment.Hi)
			}
		}
		lo, hi = chart.Extent(ends...)
	} else {
		var all []float64
		for _, series := range values {
			all = append(all, series...)
		}
		lo, hi = chart.Extent(all...)
	}
	p.y = chartScale(lo, hi, value.Type != product.ChartLine)
	p.yTicks, p.base = p.y.Ticks(4), p.y.At(0)
	n := len(value.Labels)
	if value.Type == product.ChartBar {
		p.band = chart.NewBand(value.Labels...).Padding(0.25, 0.1)
		p.xTicks = chartCategoryTicks(value.Labels, p.band.At)
		for i := range n {
			p.xs = append(p.xs, p.band.At(i))
		}
		return p
	}
	p.xs = make([]float32, n)
	if value.XValues != nil {
		x := chart.NewLinear(value.XValues[0], value.XValues[n-1])
		if n == 1 {
			x = chart.NewLinear(value.XValues[0]-1, value.XValues[0]+1)
		}
		integral := true
		for i, v := range value.XValues {
			p.xs[i] = x.At(v)
			integral = integral && v == math.Trunc(v)
		}
		// About one tick per point for short series; whole-number x such as
		// years never gets fractional ticks.
		for _, tick := range x.Ticks(max(1, min(5, n-1))) {
			if !integral || tick.Value == math.Trunc(tick.Value) {
				if integral {
					tick.Label = strconv.FormatFloat(tick.Value, 'f', 0, 64)
				}
				p.xTicks = append(p.xTicks, tick)
			}
		}
	} else {
		// Categories sit mid-step, as a band scale's, so end labels stay inside.
		for i := range n {
			p.xs[i] = (float32(i) + .5) / float32(n)
		}
		p.xTicks = chartCategoryTicks(value.Labels, func(i int) float32 { return p.xs[i] })
	}
	if value.Stacked {
		// A null takes no room in the stack, as with stacked bars, and is a
		// gap in its own band: the prompt's one rule, "null marks a gap".
		for s, series := range p.stack {
			los, his := make([]float64, n), make([]float64, n)
			for i, segment := range series {
				los[i], his[i] = segment.Lo, segment.Hi
				if math.IsNaN(values[s][i]) {
					los[i], his[i] = math.NaN(), math.NaN()
				}
			}
			p.lo, p.ys = append(p.lo, chartFractions(p.y, los)), append(p.ys, chartFractions(p.y, his))
		}
	} else {
		for _, series := range values {
			p.ys = append(p.ys, chartFractions(p.y, series))
		}
	}
	// A run of one defined point draws no line or area: mark it with a dot.
	for _, ys := range p.ys {
		var dots []int
		defined := func(i int) bool { return i >= 0 && i < len(ys) && !math.IsNaN(float64(ys[i])) }
		for i := range ys {
			if defined(i) && !defined(i-1) && !defined(i+1) {
				dots = append(dots, i)
			}
		}
		p.dots = append(p.dots, dots)
	}
	return p
}

// chartColumns reduces an area's points to the plot's pixel columns as
// chart.MinMax does for a line (the first, lowest, highest and last point of
// each column in each run) but keeps indices, so the upper and lower edges stay
// aligned; -1 separates runs of defined points.
func chartColumns(dst []int, xs, lo, hi []float32, columns int) []int {
	dst = dst[:0]
	if columns <= 0 {
		return dst
	}
	defined := func(i int) bool {
		return !math.IsNaN(float64(hi[i])) && (lo == nil || !math.IsNaN(float64(lo[i])))
	}
	keep := func(i int) {
		if len(dst) == 0 || dst[len(dst)-1] != i {
			dst = append(dst, i)
		}
	}
	for i := 0; i < len(xs); {
		if !defined(i) {
			i++
			continue
		}
		if len(dst) > 0 {
			dst = append(dst, -1)
		}
		for i < len(xs) && defined(i) {
			bucket := int(xs[i] * float32(columns))
			first, low, high, last := i, i, i, i
			for i++; i < len(xs) && defined(i) && int(xs[i]*float32(columns)) == bucket; i++ {
				if hi[i] < hi[low] {
					low = i
				}
				if hi[i] > hi[high] {
					high = i
				}
				last = i
			}
			keep(first)
			keep(min(low, high))
			keep(max(low, high))
			keep(last)
		}
	}
	return dst
}

// chartAreaInto fills between hi and lo (or the constant base) over the
// reduced indices, reusing path; runs of fewer than two points add nothing.
func chartAreaInto(path *ui.Path, r ui.Rect, idx []int, xs, lo, hi []float32, base float32) *ui.Path {
	path.Reset()
	for start := 0; start < len(idx); {
		end := start
		for end < len(idx) && idx[end] >= 0 {
			end++
		}
		if end-start >= 2 {
			for k := start; k < end; k++ {
				i := idx[k]
				if k == start {
					path.MoveTo(chart.X(r, xs[i]), chart.Y(r, hi[i]))
				} else {
					path.LineTo(chart.X(r, xs[i]), chart.Y(r, hi[i]))
				}
			}
			for k := end - 1; k >= start; k-- {
				i, bottom := idx[k], base
				if lo != nil {
					bottom = lo[i]
				}
				path.LineTo(chart.X(r, xs[i]), chart.Y(r, bottom))
			}
			path.Close()
		}
		start = end + 1
	}
	return path
}

func chartAxes(c *ui.Context, p *chartPlot) chart.Axes {
	muted := c.Theme().TextMuted
	return chart.Axes{Left: p.yTicks, Bottom: p.xTicks, Label: func(_ chart.Side, tick chart.Tick) {
		ui.Text(c, tick.Label).NoWrap().FontSize(workspaceFontCaption).TextColor(muted)
	}}
}

// chartTooltip anchors a reading near the top of the plot, kept inside it.
func chartTooltip(c *ui.Context, x float32, lines []string) {
	th := c.Theme()
	align := ui.Center
	if x < .25 {
		align = ui.Start
	} else if x > .75 {
		align = ui.End
	}
	chart.Anchor(c, x, 1, align, ui.Start, func() {
		ui.Column(c).Label("Chart reading").Padding(4, 8).Gap(2).Radius(6).Background(th.Surface).Border(1, th.Border).Children(func() {
			for _, line := range lines {
				ui.Text(c, line).NoWrap().FontSize(workspaceFontCaption).TextColor(th.Text)
			}
		})
	})
}

func (e *chartEntry) reading(i int) []string {
	value := e.chart
	lines := []string{value.Labels[i]}
	for s, series := range value.Series {
		line := chartNumber(series.Values[i], value.Unit)
		if len(value.Series) > 1 || series.Name != "" {
			line = chartSeriesName(value, s) + "：" + line
		}
		lines = append(lines, line)
	}
	return lines
}

func buildChartPlot(c *ui.Context, e *chartEntry, v *chartView, label string) {
	th := c.Theme()
	value, p := e.chart, e.plot
	describe := func(lines []string) string { return strings.Join(lines, "，") }
	grid := th.Border
	switch value.Type {
	case product.ChartPie:
		buildChartPie(c, e, v, label)
		return
	case product.ChartScatter:
		chart.Plot(c, chartAxes(c, p), func(area ui.Element) {
			area.Label("Chart plot " + label).Description(value.Title)
			area.Draw(func(painter *ui.Painter, r ui.Rect) {
				for _, tick := range p.yTicks {
					painter.Line(r.X, chart.Y(r, tick.At), r.X+r.W, chart.Y(r, tick.At), 1, grid)
				}
			})
			reading := func(i int) []string {
				ref := p.order[i]
				point := value.Series[ref.series].Points[ref.point]
				lines := []string{"x " + chartNumber(point.X, "") + "，y " + chartNumber(point.Y, value.Unit)}
				if len(value.Series) > 1 || value.Series[ref.series].Name != "" {
					lines = append([]string{chartSeriesName(value, ref.series)}, lines...)
				}
				return lines
			}
			chart.Select(c, area, &v.selected, p.orderXs, func(i int) string { return describe(reading(i)) })
			for s := range p.px {
				color := chartColor(th, s)
				for i := range p.px[s] {
					chart.Point(c, p.px[s][i], p.py[s][i], 8, 8).Radius(4).Background(color.Alpha(.85))
				}
			}
			if i := v.selected; i >= 0 && i < len(p.order) {
				ref := p.order[i]
				x, y := p.px[ref.series][ref.point], p.py[ref.series][ref.point]
				chart.Point(c, x, y, 14, 14).Radius(7).Border(2, th.Text).Background(chartColor(th, ref.series))
				chartTooltip(c, x, reading(i))
			}
		}).Height(chartPlotHeight).Gap(6).FillWidth()
		return
	case product.ChartBar:
		chart.Plot(c, chartAxes(c, p), func(area ui.Element) {
			area.Label("Chart plot " + label).Description(value.Title)
			area.Draw(func(painter *ui.Painter, r ui.Rect) {
				for _, tick := range p.yTicks {
					painter.Line(r.X, chart.Y(r, tick.At), r.X+r.W, chart.Y(r, tick.At), 1, grid)
				}
			})
			chart.Select(c, area, &v.selected, p.xs, func(i int) string { return describe(e.reading(i)) })
			if i := v.selected; i >= 0 && i < len(p.xs) {
				step := p.band.Band(i)
				pad := p.band.Width() * .25
				chart.Rect(c, chart.Span{Lo: max(0, step.Lo-pad), Hi: min(1, step.Hi+pad)}, chart.Span{Lo: 0, Hi: 1}).Radius(4).Background(th.Text.Alpha(.06))
			}
			series := len(value.Series)
			radius := float32(0)
			if len(value.Labels)*series <= 24 {
				radius = 3
			}
			for s := range value.Series {
				color := chartColor(th, s)
				for i, x := range value.Series[s].Values {
					if math.IsNaN(x) {
						continue
					}
					band := p.band.Band(i)
					span, ys := band, p.y.Span(0, x)
					if value.Stacked {
						segment := p.stack[s][i]
						if segment.Lo == segment.Hi {
							continue
						}
						ys = p.y.Span(segment.Lo, segment.Hi)
					} else {
						width := (band.Hi - band.Lo) / float32(series)
						span = chart.Span{Lo: band.Lo + width*float32(s), Hi: band.Lo + width*float32(s+1)}
						if series > 1 {
							span.Hi -= width * .1
						}
					}
					chart.Rect(c, span, ys).Radius(radius).Background(color)
				}
			}
			if i := v.selected; i >= 0 && i < len(p.xs) {
				chartTooltip(c, p.xs[i], e.reading(i))
			}
		}).Height(chartPlotHeight).Gap(6).FillWidth()
		return
	}
	// Line and area: one Draw per plot strokes every series from fractions
	// reduced to the plot's pixel columns.
	area := value.Type == product.ChartArea
	colors := make([]ui.Color, len(p.ys))
	for s := range colors {
		colors[s] = chartColor(th, s)
	}
	chart.Plot(c, chartAxes(c, p), func(plot ui.Element) {
		plot.Label("Chart plot " + label).Description(value.Title)
		plot.Draw(func(painter *ui.Painter, r ui.Rect) {
			for _, tick := range p.yTicks {
				painter.Line(r.X, chart.Y(r, tick.At), r.X+r.W, chart.Y(r, tick.At), 1, grid)
			}
			columns := int(math.Ceil(float64(r.W * painter.Scale())))
			for s, ys := range p.ys {
				if area {
					var lo []float32
					if p.lo != nil {
						lo = p.lo[s]
					}
					p.idx = chartColumns(p.idx, p.xs, lo, ys, columns)
					painter.FillPath(chartAreaInto(&p.fill, r, p.idx, p.xs, lo, ys, p.base), colors[s].Alpha(.22))
					// The upper edge follows the same reduced points.
					p.bx, p.by = p.bx[:0], p.by[:0]
					for _, i := range p.idx {
						if i < 0 {
							p.bx, p.by = append(p.bx, float32(math.NaN())), append(p.by, float32(math.NaN()))
						} else {
							p.bx, p.by = append(p.bx, p.xs[i]), append(p.by, ys[i])
						}
					}
				} else {
					p.bx, p.by = chart.MinMax(p.bx, p.by, p.xs, ys, columns)
				}
				painter.StrokePath(chart.LineInto(&p.path, r, p.bx, p.by, chart.CurveLinear), 2, colors[s])
				for _, i := range p.dots[s] {
					painter.FillPath(p.path.Reset().Circle(chart.X(r, p.xs[i]), chart.Y(r, ys[i]), 3), colors[s])
				}
			}
		})
		chart.Select(c, plot, &v.selected, p.xs, func(i int) string { return describe(e.reading(i)) })
		if i := v.selected; i >= 0 && i < len(p.xs) {
			chart.RuleX(c, p.xs[i], 1).Background(th.TextMuted)
			for s, ys := range p.ys {
				if !math.IsNaN(float64(ys[i])) && !math.IsNaN(value.Series[s].Values[i]) {
					chart.Point(c, p.xs[i], ys[i], 9, 9).Radius(4.5).Border(2, th.Surface).Background(colors[s])
				}
			}
			chartTooltip(c, p.xs[i], e.reading(i))
		}
	}).Height(chartPlotHeight).Gap(6).FillWidth()
}

type chartPieHover struct {
	over bool
	x, y float32
}

// buildChartPie draws the sectors beside a value legend. Past the seventh
// slice, sectors share one neutral color and the legend folds them into
// "其他" rather than cycling hues; hovering or arrows still read each one.
func buildChartPie(c *ui.Context, e *chartEntry, v *chartView, label string) {
	th := c.Theme()
	value, p := e.chart, e.plot
	values := value.Series[0].Values
	const shown = 7
	color := func(i int) ui.Color {
		if len(values) > shown+1 && i >= shown {
			return th.TextMuted.Alpha(.45)
		}
		return chartColor(th, i)
	}
	reading := func(i int) string {
		share := 0.0
		if p.total > 0 {
			share = values[i] / p.total * 100
		}
		return fmt.Sprintf("%s：%s（%.1f%%）", value.Labels[i], chartNumber(values[i], value.Unit), share)
	}
	ui.Row(c).Height(chartPlotHeight).Gap(24).AlignItems(ui.Center).Children(func() {
		ring := ui.Box(c).Label("Chart plot "+label).Description(value.Title).Size(chartPieSize, chartPieSize).Shrink(0).Focusable()
		if v.selected >= len(values) {
			v.selected = -1
		}
		// As chart.Select: arrows move the choice, the pointer chooses only as
		// it comes and moves (not while it rests over a keyed choice), and
		// leaving clears it.
		keyed := true
		switch {
		case ring.Shortcut(0, ui.KeyRight):
			v.selected = (v.selected + 1) % len(values)
		case ring.Shortcut(0, ui.KeyLeft):
			if v.selected <= 0 {
				v.selected = len(values) - 1
			} else {
				v.selected--
			}
		case v.selected >= 0 && ring.Shortcut(0, ui.KeyEscape):
			v.selected = -1
		default:
			keyed = false
		}
		const outer = chartPieSize/2 - 6
		hover := ui.Local(ring, "chart.pie", func() chartPieHover { return chartPieHover{} })
		if x, y, over := ring.PointerPosition(); over {
			if !keyed && (!hover.over || x != hover.x || y != hover.y) {
				v.selected = -1
				if i, ok := chart.SliceAt(p.slices, x-chartPieSize/2, y-chartPieSize/2, 0, outer+6); ok {
					v.selected = i
				}
			}
			hover.over, hover.x, hover.y = true, x, y
		} else if hover.over {
			hover.over = false
			v.selected = -1
		}
		if v.selected >= 0 {
			ring.Value(reading(v.selected))
			if keyed {
				c.Announce(reading(v.selected))
			}
		}
		selected := v.selected
		colors := make([]ui.Color, len(values))
		for i := range colors {
			colors[i] = color(i)
		}
		ring.Draw(func(painter *ui.Painter, r ui.Rect) {
			cx, cy := r.X+r.W/2, r.Y+r.H/2
			for i, slice := range p.slices {
				radius := float32(outer)
				if i == selected {
					radius += 5
				}
				painter.FillPath(chart.Sector(cx, cy, 0, radius, slice), colors[i])
			}
		})
		ui.Column(c).Label("Chart legend").Gap(4).MinWidth(0).Grow(1).Children(func() {
			for i := range min(len(values), shown+1) {
				text := reading(i)
				if len(values) > shown+1 && i == shown {
					rest := 0.0
					for _, x := range values[shown:] {
						rest += x
					}
					text = fmt.Sprintf("其他 %d 项：%s（%.1f%%）", len(values)-shown, chartNumber(rest, value.Unit), rest/p.total*100)
				}
				ui.Row(c).Gap(6).AlignItems(ui.Center).Height(20).Children(func() {
					ui.Box(c).Size(10, 10).Radius(3).Background(colors[i]).Shrink(0)
					weight := 400
					if i == selected {
						weight = 600
					}
					ui.Text(c, text).FontSize(workspaceFontCaption).FontWeight(weight).TextColor(th.Text).SingleLine().MinWidth(0)
				})
			}
			// A folded slice (the eighth on) is read out below the legend.
			if selected >= shown && len(values) > shown+1 {
				ui.Row(c).Label("Chart reading").Children(func() {
					ui.Text(c, reading(selected)).FontSize(workspaceFontCaption).TextColor(th.TextMuted).SingleLine()
				})
			}
		})
	})
}
