// INPUT: encoding/json tokens, math, strconv, unicode/utf8 and the SDK Plugin registrar
// OUTPUT: Chart/ChartSeries/ChartPoint/ChartType, ParseChart, ChartInstructions and ChartBlocksPlugin
// POS: [L7 Hub product] The one chart data-block format: strict pure parser, its limits and the only model-facing description kept together
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/agent/core"
)

// ChartInstructions is the only description of the chart data block given to
// models. It sits beside ParseChart so the format and its prompt change together.
const ChartInstructions = "Charts: when a chart clearly helps, put one JSON object in a fenced code block tagged chart. Example:\n" +
	"```chart\n" +
	`{"type":"line","title":"Sales","x":["Jan","Feb","Mar"],"series":[{"name":"A","data":[12,null,9]}],"unit":"k"}` + "\n" +
	"```\n" +
	"type: bar, line, area, pie or scatter. x lists labels or ascending numbers; each series data aligns with x, null marks a gap. " +
	"pie: one series, non-negative values. scatter: no x; data is [[x,y],...]. Optional: title, unit, stacked (bar/area). " +
	"Limits: 8 series, 100 bar/pie categories, 2000 line/area points, 500 scatter points, 80-byte labels, 32 KiB, |numbers| <= 1e300. Literal numbers only; no other fields."

// ChartType names one of the supported chart forms.
type ChartType string

const (
	ChartBar     ChartType = "bar"
	ChartLine    ChartType = "line"
	ChartArea    ChartType = "area"
	ChartPie     ChartType = "pie"
	ChartScatter ChartType = "scatter"
)

// Format limits. A chart over any of them is rejected rather than truncated.
const (
	ChartMaxBytes         = 32 << 10
	ChartMaxSeries        = 8
	ChartMaxCategories    = 100  // bar and pie categories
	ChartMaxLinePoints    = 2000 // line and area points per series
	ChartMaxScatterPoints = 500  // scatter points across all series
	ChartMaxLabelBytes    = 80
	// ChartMaxMagnitude keeps sums, stacks and axis spans of up to 100 values
	// far from float64 overflow.
	ChartMaxMagnitude = 1e300
)

// Chart is validated chart data. It holds only numbers and short labels; no
// field is an expression, a URL or anything else that could be executed.
type Chart struct {
	Type    ChartType
	Title   string
	Unit    string
	Stacked bool
	// Labels are the x categories of bar/line/area and the sectors of pie.
	// Numeric x values are also kept, formatted, as labels.
	Labels []string
	// XValues holds numeric x positions, strictly ascending, when x was given
	// as numbers; nil when x was text.
	XValues []float64
	Series  []ChartSeries
}

// ChartSeries is one named series. Values align with Chart.Labels, and a NaN
// in it stands only for an input null (a gap). Points is used by scatter only.
type ChartSeries struct {
	Name   string
	Values []float64
	Points []ChartPoint
}

type ChartPoint struct{ X, Y float64 }

// ChartBlocksPlugin contributes ChartInstructions as a separate system prompt
// part. It never touches the session's own SystemPrompt or its budget.
type ChartBlocksPlugin struct{}

func (ChartBlocksPlugin) Name() string { return "hub-chart-blocks" }
func (ChartBlocksPlugin) Register(_ context.Context, r *core.Registrar) error {
	return r.SystemPrompt(ChartInstructions)
}

// ParseChart is pure: it reads the text of one chart code block and returns the
// chart or a short, user-readable reason. Unknown and duplicate fields are
// rejected so that a misspelled option never silently draws something else.
func ParseChart(source string) (Chart, error) {
	if len(source) > ChartMaxBytes {
		return Chart{}, errors.New("图表数据超过 32 KiB")
	}
	if !utf8.ValidString(source) {
		return Chart{}, errors.New("图表数据不是有效的 UTF-8 文本")
	}
	text := strings.TrimSpace(source)
	if text == "" {
		return Chart{}, errors.New("图表数据为空")
	}
	p := chartParser{d: json.NewDecoder(strings.NewReader(text))}
	p.d.UseNumber()
	raw, err := p.object()
	if err != nil {
		return Chart{}, err
	}
	if _, err := p.d.Token(); err != io.EOF {
		return Chart{}, errors.New("JSON 对象之后还有多余内容")
	}
	return raw.validate()
}

type chartValue struct {
	number float64
	null   bool
	pair   bool
	x, y   float64
}

type rawSeries struct {
	name    string
	hasData bool
	data    []chartValue
}

type rawChart struct {
	typ, title, unit string
	hasType, stacked bool
	hasX, hasSeries  bool
	xLabels          []string
	xNumbers         []float64
	series           []rawSeries
	textX, numericX  bool
}

type chartParser struct{ d *json.Decoder }

// token maps decoder failures to one readable reason; positions inside a
// 32 KiB block add nothing a reader could act on.
func (p chartParser) token() (json.Token, error) {
	token, err := p.d.Token()
	if err != nil {
		if err == io.EOF || err == io.ErrUnexpectedEOF || strings.Contains(err.Error(), "unexpected end of JSON input") {
			return nil, errors.New("JSON 不完整")
		}
		return nil, errors.New("不是有效的 JSON")
	}
	return token, nil
}

func (p chartParser) delim(want json.Delim, what string) error {
	token, err := p.token()
	if err != nil {
		return err
	}
	if token != want {
		if want == '{' {
			return fmt.Errorf("%s应为对象", what)
		}
		return fmt.Errorf("%s应为数组", what)
	}
	return nil
}

func (p chartParser) str(field string) (string, error) {
	token, err := p.token()
	if err != nil {
		return "", err
	}
	text, ok := token.(string)
	if !ok {
		return "", fmt.Errorf("%s 应为字符串", field)
	}
	return text, nil
}

func (p chartParser) object() (rawChart, error) {
	var raw rawChart
	if err := p.delim('{', "图表数据"); err != nil {
		return raw, err
	}
	seen := map[string]bool{}
	for p.d.More() {
		key, err := p.key(seen)
		if err != nil {
			return raw, err
		}
		switch key {
		case "type":
			raw.typ, err = p.str("type")
			raw.hasType = true
		case "title":
			raw.title, err = p.str("title")
		case "unit":
			raw.unit, err = p.str("unit")
		case "stacked":
			var token json.Token
			if token, err = p.token(); err == nil {
				value, ok := token.(bool)
				if !ok {
					err = errors.New("stacked 应为 true 或 false")
				}
				raw.stacked = value
			}
		case "x":
			raw.hasX = true
			err = p.x(&raw)
		case "series":
			raw.hasSeries = true
			err = p.seriesList(&raw)
		default:
			err = fmt.Errorf("不支持的字段 %s", key)
		}
		if err != nil {
			return raw, err
		}
	}
	if _, err := p.token(); err != nil {
		return raw, err
	}
	return raw, nil
}

func (p chartParser) key(seen map[string]bool) (string, error) {
	token, err := p.token()
	if err != nil {
		return "", err
	}
	key, _ := token.(string)
	if seen[key] {
		return "", fmt.Errorf("字段 %s 重复", key)
	}
	seen[key] = true
	return key, nil
}

func (p chartParser) x(raw *rawChart) error {
	if err := p.delim('[', "x "); err != nil {
		return err
	}
	for p.d.More() {
		if len(raw.xLabels) >= ChartMaxLinePoints {
			return fmt.Errorf("x 最多 %d 项", ChartMaxLinePoints)
		}
		token, err := p.token()
		if err != nil {
			return err
		}
		switch value := token.(type) {
		case string:
			raw.textX = true
			raw.xLabels = append(raw.xLabels, value)
		case json.Number:
			number, err := chartNumber(value, fmt.Sprintf("x 的第 %d 项", len(raw.xLabels)+1))
			if err != nil {
				return err
			}
			raw.numericX = true
			raw.xNumbers = append(raw.xNumbers, number)
			raw.xLabels = append(raw.xLabels, strconv.FormatFloat(number, 'g', -1, 64))
		default:
			return fmt.Errorf("x 的第 %d 项应为文字或数字", len(raw.xLabels)+1)
		}
	}
	_, err := p.token()
	return err
}

func (p chartParser) seriesList(raw *rawChart) error {
	if err := p.delim('[', "series "); err != nil {
		return err
	}
	for p.d.More() {
		if len(raw.series) >= ChartMaxSeries {
			return fmt.Errorf("series 最多 %d 个", ChartMaxSeries)
		}
		index := len(raw.series) + 1
		if err := p.delim('{', fmt.Sprintf("series %d ", index)); err != nil {
			return err
		}
		var series rawSeries
		seen := map[string]bool{}
		for p.d.More() {
			key, err := p.key(seen)
			if err != nil {
				return fmt.Errorf("series %d：%w", index, err)
			}
			switch key {
			case "name":
				series.name, err = p.str(fmt.Sprintf("series %d 的 name", index))
			case "data":
				series.hasData = true
				series.data, err = p.data(index)
			default:
				err = fmt.Errorf("series %d 有不支持的字段 %s", index, key)
			}
			if err != nil {
				return err
			}
		}
		if _, err := p.token(); err != nil {
			return err
		}
		raw.series = append(raw.series, series)
	}
	_, err := p.token()
	return err
}

func (p chartParser) data(index int) ([]chartValue, error) {
	if err := p.delim('[', fmt.Sprintf("series %d 的 data ", index)); err != nil {
		return nil, err
	}
	var values []chartValue
	for p.d.More() {
		if len(values) >= ChartMaxLinePoints {
			return nil, fmt.Errorf("series %d 的数据最多 %d 个", index, ChartMaxLinePoints)
		}
		where := fmt.Sprintf("series %d 的第 %d 个值", index, len(values)+1)
		token, err := p.token()
		if err != nil {
			return nil, err
		}
		switch value := token.(type) {
		case nil:
			values = append(values, chartValue{null: true})
		case json.Number:
			number, err := chartNumber(value, where)
			if err != nil {
				return nil, err
			}
			values = append(values, chartValue{number: number})
		case json.Delim:
			if value != '[' {
				return nil, fmt.Errorf("%s应为数字、null 或 [x,y]", where)
			}
			var pair [2]float64
			for i := range pair {
				token, err := p.token()
				if err != nil {
					return nil, err
				}
				number, ok := token.(json.Number)
				if !ok {
					return nil, fmt.Errorf("%s应为两个数字 [x,y]", where)
				}
				if pair[i], err = chartNumber(number, where); err != nil {
					return nil, err
				}
			}
			if token, err := p.token(); err != nil || token != json.Delim(']') {
				return nil, fmt.Errorf("%s应为两个数字 [x,y]", where)
			}
			values = append(values, chartValue{pair: true, x: pair[0], y: pair[1]})
		default:
			return nil, fmt.Errorf("%s应为数字或 null", where)
		}
	}
	_, err := p.token()
	return values, err
}

// chartNumber rejects literals that overflow to infinity; JSON itself cannot
// spell NaN or Infinity, so every accepted value is a finite float64. Values
// past ±1e300 are refused too, so ranges, stacks and pie totals stay finite.
func chartNumber(value json.Number, where string) (float64, error) {
	number, err := strconv.ParseFloat(string(value), 64)
	if err != nil || math.IsNaN(number) || math.IsInf(number, 0) {
		return 0, fmt.Errorf("%s不是有限数", where)
	}
	if math.Abs(number) > ChartMaxMagnitude {
		return 0, fmt.Errorf("%s超出 ±1e300", where)
	}
	return number, nil
}

func chartLabel(text, what string) error {
	if len(text) > ChartMaxLabelBytes {
		return fmt.Errorf("%s超过 %d 字节", what, ChartMaxLabelBytes)
	}
	if strings.ContainsAny(text, "\x00\r\n") {
		return fmt.Errorf("%s不能包含换行或空字符", what)
	}
	// Bidirectional controls can make a label display as different text.
	if strings.ContainsAny(text, "\u061c\u200e\u200f\u202a\u202b\u202c\u202d\u202e\u2066\u2067\u2068\u2069") {
		return fmt.Errorf("%s不能包含双向文字控制字符", what)
	}
	return nil
}

func (raw rawChart) validate() (Chart, error) {
	if !raw.hasType {
		return Chart{}, errors.New("缺少 type")
	}
	chart := Chart{Type: ChartType(raw.typ), Title: raw.title, Unit: raw.unit, Stacked: raw.stacked}
	switch chart.Type {
	case ChartBar, ChartLine, ChartArea, ChartPie, ChartScatter:
	default:
		return Chart{}, fmt.Errorf("不支持的图表类型 %q，应为 bar、line、area、pie 或 scatter", raw.typ)
	}
	if err := chartLabel(raw.title, "title "); err != nil {
		return Chart{}, err
	}
	if err := chartLabel(raw.unit, "unit "); err != nil {
		return Chart{}, err
	}
	if raw.stacked && chart.Type != ChartBar && chart.Type != ChartArea {
		return Chart{}, errors.New("stacked 只适用于 bar 和 area")
	}
	if !raw.hasSeries {
		return Chart{}, errors.New("缺少 series")
	}
	if len(raw.series) == 0 {
		return Chart{}, errors.New("series 不能为空")
	}
	for i, series := range raw.series {
		if err := chartLabel(series.name, fmt.Sprintf("series %d 的 name ", i+1)); err != nil {
			return Chart{}, err
		}
		if !series.hasData {
			return Chart{}, fmt.Errorf("series %d 缺少 data", i+1)
		}
		if len(series.data) == 0 {
			return Chart{}, fmt.Errorf("series %d 的 data 不能为空", i+1)
		}
	}
	if chart.Type == ChartScatter {
		return raw.scatter(chart)
	}
	if !raw.hasX {
		return Chart{}, errors.New("缺少 x")
	}
	if len(raw.xLabels) == 0 {
		return Chart{}, errors.New("x 不能为空")
	}
	if raw.textX && raw.numericX {
		return Chart{}, errors.New("x 不能混用文字和数字")
	}
	limit := ChartMaxCategories
	if chart.Type == ChartLine || chart.Type == ChartArea {
		limit = ChartMaxLinePoints
	}
	if len(raw.xLabels) > limit {
		return Chart{}, fmt.Errorf("%s 图最多 %d 个类别，当前 %d 个", chart.Type, limit, len(raw.xLabels))
	}
	for i, label := range raw.xLabels {
		if err := chartLabel(label, fmt.Sprintf("x 的第 %d 项", i+1)); err != nil {
			return Chart{}, err
		}
	}
	chart.Labels = append([]string(nil), raw.xLabels...)
	if raw.numericX && chart.Type != ChartPie {
		for i := 1; i < len(raw.xNumbers); i++ {
			if raw.xNumbers[i] <= raw.xNumbers[i-1] {
				return Chart{}, errors.New("数字形式的 x 必须严格递增")
			}
		}
		chart.XValues = append([]float64(nil), raw.xNumbers...)
	}
	if chart.Type == ChartPie && len(raw.series) != 1 {
		return Chart{}, fmt.Errorf("pie 只能有一个 series，当前 %d 个", len(raw.series))
	}
	drawable := false
	for i, series := range raw.series {
		if len(series.data) != len(chart.Labels) {
			return Chart{}, fmt.Errorf("series %d 的数据个数和 x 不一致（%d 对 %d）", i+1, len(series.data), len(chart.Labels))
		}
		values := make([]float64, len(series.data))
		for j, value := range series.data {
			switch {
			case value.pair:
				return Chart{}, fmt.Errorf("series %d 的第 %d 个值应为数字或 null，[x,y] 只用于 scatter", i+1, j+1)
			case value.null && chart.Type == ChartPie:
				return Chart{}, fmt.Errorf("pie 的第 %d 个值不能为 null", j+1)
			case value.null:
				values[j] = math.NaN()
			case chart.Type == ChartPie && value.number < 0:
				return Chart{}, fmt.Errorf("pie 的第 %d 个值不能为负数", j+1)
			default:
				values[j] = value.number
				drawable = drawable || value.number != 0 || chart.Type != ChartPie
			}
		}
		chart.Series = append(chart.Series, ChartSeries{Name: series.name, Values: values})
	}
	if !drawable {
		if chart.Type == ChartPie {
			return Chart{}, errors.New("pie 的数值之和必须大于 0")
		}
		return Chart{}, errors.New("没有可绘制的数值，全部为 null")
	}
	return chart, nil
}

func (raw rawChart) scatter(chart Chart) (Chart, error) {
	if raw.hasX {
		return Chart{}, errors.New("scatter 不使用 x，点写成 series 的 data：[[x,y],…]")
	}
	total := 0
	for i, series := range raw.series {
		points := make([]ChartPoint, len(series.data))
		for j, value := range series.data {
			if !value.pair {
				return Chart{}, fmt.Errorf("series %d 的第 %d 个值应为 [x,y]", i+1, j+1)
			}
			points[j] = ChartPoint{X: value.x, Y: value.y}
		}
		total += len(points)
		if total > ChartMaxScatterPoints {
			return Chart{}, fmt.Errorf("scatter 最多 %d 个点", ChartMaxScatterPoints)
		}
		chart.Series = append(chart.Series, ChartSeries{Name: series.name, Points: points})
	}
	return chart, nil
}
