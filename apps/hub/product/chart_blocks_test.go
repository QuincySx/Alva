// INPUT: ParseChart, ChartInstructions and ChartBlocksPlugin with literal chart block texts
// OUTPUT: Valid sample per type, every limit, non-finite numbers, shape/emptiness/unknown-field rejection and prompt registration
// POS: [L7 Hub product tests] The chart format and its model-facing description stay one checked contract
package product

import (
	"context"
	"fmt"
	"math"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func chartNumbers(n int, value func(i int) string) string {
	parts := make([]string, n)
	for i := range parts {
		parts[i] = value(i)
	}
	return strings.Join(parts, ",")
}

func chartLabels(n int) string {
	return chartNumbers(n, func(i int) string { return fmt.Sprintf("%q", fmt.Sprintf("c%d", i)) })
}

func TestParseChartAcceptsEachType(t *testing.T) {
	bar, err := ParseChart(`{"type":"bar","title":"月销售额","x":["1月","2月","3月"],"series":[{"name":"A","data":[12,18,9]},{"name":"B","data":[3,null,-4]}],"unit":"万元","stacked":true}`)
	if err != nil || bar.Type != ChartBar || !bar.Stacked || bar.Title != "月销售额" || bar.Unit != "万元" || len(bar.Labels) != 3 || len(bar.Series) != 2 || !math.IsNaN(bar.Series[1].Values[1]) || bar.Series[1].Values[2] != -4 || bar.XValues != nil {
		t.Fatal("bar", bar, err)
	}
	line, err := ParseChart("\n" + `{"type":"line","x":[2020,2021.5,2023],"series":[{"data":[1,2,3]}]}` + "\n")
	if err != nil || line.Type != ChartLine || len(line.XValues) != 3 || line.XValues[1] != 2021.5 || line.Labels[1] != "2021.5" || line.Series[0].Name != "" {
		t.Fatal("line", line, err)
	}
	area, err := ParseChart(`{"type":"area","x":["a","b"],"series":[{"name":"s","data":[null,2]}],"stacked":false}`)
	if err != nil || area.Type != ChartArea || area.Stacked || !math.IsNaN(area.Series[0].Values[0]) {
		t.Fatal("area", area, err)
	}
	pie, err := ParseChart(`{"type":"pie","x":["macOS","Windows","Linux"],"series":[{"name":"用户","data":[50,30,0]}]}`)
	if err != nil || pie.Type != ChartPie || len(pie.Labels) != 3 || pie.Labels[2] != "Linux" || pie.XValues != nil {
		t.Fatal("pie", pie, err)
	}
	numericPie, err := ParseChart(`{"type":"pie","x":[3,1],"series":[{"data":[1,2]}]}`)
	if err != nil || numericPie.Labels[0] != "3" || numericPie.XValues != nil {
		t.Fatal("pie", pie, err)
	}
	scatter, err := ParseChart(`{"type":"scatter","title":"身高体重","series":[{"name":"A","data":[[1.5,2],[3,-4e2]]},{"name":"B","data":[[0,0]]}]}`)
	if err != nil || scatter.Type != ChartScatter || len(scatter.Series) != 2 || scatter.Series[0].Points[1] != (ChartPoint{3, -400}) || scatter.Labels != nil {
		t.Fatal("scatter", scatter, err)
	}
}

func TestParseChartEnforcesEveryLimit(t *testing.T) {
	series := func(n int) string {
		return chartNumbers(n, func(int) string { return `{"data":[1]}` })
	}
	long := strings.Repeat("长", 27) // 81 bytes
	cases := map[string]struct{ text, want string }{
		"bytes":          {`{"type":"bar","title":"` + strings.Repeat("a", ChartMaxBytes) + `"}`, "超过 32 KiB"},
		"series":         {`{"type":"bar","x":["a"],"series":[` + series(9) + `]}`, "series 最多 8 个"},
		"bar categories": {`{"type":"bar","x":[` + chartLabels(101) + `],"series":[{"data":[` + chartNumbers(101, func(int) string { return "1" }) + `]}]}`, "bar 图最多 100 个类别"},
		"pie categories": {`{"type":"pie","x":[` + chartLabels(101) + `],"series":[{"data":[` + chartNumbers(101, func(int) string { return "1" }) + `]}]}`, "pie 图最多 100 个类别"},
		"line points":    {`{"type":"line","x":[` + chartNumbers(2001, func(i int) string { return fmt.Sprint(i) }) + `],"series":[{"data":[1]}]}`, "x 最多 2000 项"},
		"area points":    {`{"type":"area","x":[1],"series":[{"data":[` + chartNumbers(2001, func(int) string { return "1" }) + `]}]}`, "数据最多 2000 个"},
		"scatter points": {`{"type":"scatter","series":[{"data":[` + chartNumbers(300, func(i int) string { return fmt.Sprintf("[%d,1]", i) }) + `]},{"data":[` + chartNumbers(201, func(i int) string { return fmt.Sprintf("[%d,2]", i) }) + `]}]}`, "scatter 最多 500 个点"},
		"title label":    {`{"type":"bar","title":"` + long + `","x":["a"],"series":[{"data":[1]}]}`, "title 超过 80 字节"},
		"unit label":     {`{"type":"bar","unit":"` + long + `","x":["a"],"series":[{"data":[1]}]}`, "unit 超过 80 字节"},
		"x label":        {`{"type":"bar","x":["a","` + long + `"],"series":[{"data":[1,2]}]}`, "x 的第 2 项超过 80 字节"},
		"series label":   {`{"type":"bar","x":["a"],"series":[{"name":"` + long + `","data":[1]}]}`, "series 1 的 name 超过 80 字节"},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := ParseChart(tc.text); err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("got %v, want %q", err, tc.want)
			}
		})
	}
	// Exactly at each limit is accepted.
	atLimit := []string{
		`{"type":"bar","x":[` + chartLabels(100) + `],"series":[` + chartNumbers(8, func(int) string {
			return `{"data":[` + chartNumbers(100, func(int) string { return "1" }) + `]}`
		}) + `]}`,
		`{"type":"line","x":[` + chartNumbers(2000, func(i int) string { return fmt.Sprint(i) }) + `],"series":[{"data":[` + chartNumbers(2000, func(i int) string { return fmt.Sprint(i % 7) }) + `]}]}`,
		`{"type":"scatter","series":[{"data":[` + chartNumbers(500, func(i int) string { return fmt.Sprintf("[%d,1]", i) }) + `]}]}`,
		`{"type":"bar","title":"` + strings.Repeat("a", 80) + `","x":["a"],"series":[{"data":[1]}]}`,
		`{"type":"bar","stacked":true,"x":["a","b"],"series":[{"data":[1e300,-1e300]},{"data":[1e300,-1e300]}]}`,
	}
	for i, text := range atLimit {
		if len(text) > ChartMaxBytes {
			t.Fatal("at-limit fixture itself exceeds the byte limit", i, len(text))
		}
		if _, err := ParseChart(text); err != nil {
			t.Fatal("at-limit chart rejected", i, err)
		}
	}
}

func TestParseChartRejectsMalformedData(t *testing.T) {
	cases := map[string]struct{ text, want string }{
		"overflow number":  {`{"type":"bar","x":["a"],"series":[{"data":[1e400]}]}`, "series 1 的第 1 个值不是有限数"},
		"overflow x":       {`{"type":"line","x":[1,-1e999],"series":[{"data":[1,2]}]}`, "x 的第 2 项不是有限数"},
		"overflow scatter": {`{"type":"scatter","series":[{"data":[[1,2],[3,1e309]]}]}`, "series 1 的第 2 个值不是有限数"},
		"NaN literal":      {`{"type":"bar","x":["a"],"series":[{"data":[NaN]}]}`, "不是有效的 JSON"},
		"string number":    {`{"type":"bar","x":["a"],"series":[{"data":["1"]}]}`, "series 1 的第 1 个值应为数字或 null"},
		"length mismatch":  {`{"type":"line","x":["a","b","c"],"series":[{"data":[1,2,3]},{"data":[1,2]}]}`, "series 2 的数据个数和 x 不一致"},
		"empty series":     {`{"type":"bar","x":["a"],"series":[]}`, "series 不能为空"},
		"missing series":   {`{"type":"bar","x":["a"]}`, "缺少 series"},
		"empty data":       {`{"type":"bar","x":["a"],"series":[{"name":"A","data":[]}]}`, "series 1 的 data 不能为空"},
		"all null":         {`{"type":"line","x":["a","b"],"series":[{"data":[null,null]}]}`, "没有可绘制的数值"},
		"unknown type":     {`{"type":"radar","x":["a"],"series":[{"data":[1]}]}`, "不支持的图表类型 \"radar\""},
		"missing type":     {`{"x":["a"],"series":[{"data":[1]}]}`, "缺少 type"},
		"unknown field":    {`{"type":"bar","x":["a"],"series":[{"data":[1]}],"colors":["red"]}`, "不支持的字段 colors"},
		"unknown series":   {`{"type":"bar","x":["a"],"series":[{"data":[1],"color":"red"}]}`, "series 1 有不支持的字段 color"},
		"duplicate key":    {`{"type":"bar","type":"line","x":["a"],"series":[{"data":[1]}]}`, "字段 type 重复"},
		"trailing":         {`{"type":"bar","x":["a"],"series":[{"data":[1]}]} {}`, "多余内容"},
		"truncated":        {`{"type":"bar","x":["a"],"series":[{"data":[1`, "JSON 不完整"},
		"not object":       {`[1,2]`, "图表数据应为对象"},
		"mixed x":          {`{"type":"bar","x":["a",2],"series":[{"data":[1,2]}]}`, "x 不能混用文字和数字"},
		"unsorted x":       {`{"type":"line","x":[3,2],"series":[{"data":[1,2]}]}`, "必须严格递增"},
		"stacked line":     {`{"type":"line","x":["a"],"series":[{"data":[1]}],"stacked":true}`, "stacked 只适用于 bar 和 area"},
		"pie two series":   {`{"type":"pie","x":["a"],"series":[{"data":[1]},{"data":[2]}]}`, "pie 只能有一个 series"},
		"pie negative":     {`{"type":"pie","x":["a","b"],"series":[{"data":[1,-2]}]}`, "不能为负数"},
		"pie null":         {`{"type":"pie","x":["a","b"],"series":[{"data":[1,null]}]}`, "不能为 null"},
		"pie zero sum":     {`{"type":"pie","x":["a","b"],"series":[{"data":[0,0]}]}`, "之和必须大于 0"},
		"scatter x":        {`{"type":"scatter","x":[1],"series":[{"data":[[1,2]]}]}`, "scatter 不使用 x"},
		"scatter scalar":   {`{"type":"scatter","series":[{"data":[[1,2],3]}]}`, "series 1 的第 2 个值应为 [x,y]"},
		"pair in bar":      {`{"type":"bar","x":["a"],"series":[{"data":[[1,2]]}]}`, "[x,y] 只用于 scatter"},
		"short pair":       {`{"type":"scatter","series":[{"data":[[1]]}]}`, "应为两个数字"},
		"newline label":    {`{"type":"bar","x":["a\nb"],"series":[{"data":[1]}]}`, "不能包含换行"},
		"missing x":        {`{"type":"bar","series":[{"data":[1]}]}`, "缺少 x"},
		"empty text":       {"  \n", "图表数据为空"},
		"invalid utf8":     {"{\"type\":\"bar\xff\"}", "UTF-8"},
		"huge value":       {`{"type":"bar","x":["a","b"],"series":[{"data":[1,-1.7e308]}]}`, "series 1 的第 2 个值超出 ±1e300"},
		"huge stack":       {`{"type":"bar","stacked":true,"x":["a"],"series":[{"data":[1.7e308]},{"data":[1.7e308]}]}`, "超出 ±1e300"},
		"huge x":           {`{"type":"line","x":[-1.7e308,1.7e308],"series":[{"data":[1,2]}]}`, "x 的第 1 项超出 ±1e300"},
		"huge scatter":     {`{"type":"scatter","series":[{"data":[[0,1],[2e300,2]]}]}`, "series 1 的第 2 个值超出 ±1e300"},
		"huge pie":         {`{"type":"pie","x":["a","b"],"series":[{"data":[1.7e308,1.7e308]}]}`, "超出 ±1e300"},
		"bidi title":       {"{\"type\":\"bar\",\"title\":\"\u202eabc\",\"x\":[\"a\"],\"series\":[{\"data\":[1]}]}", "title 不能包含双向文字控制字符"},
		"bidi isolate x":   {"{\"type\":\"bar\",\"x\":[\"a\u2066b\"],\"series\":[{\"data\":[1]}]}", "x 的第 1 项不能包含双向文字控制字符"},
		"bidi mark name":   {"{\"type\":\"bar\",\"x\":[\"a\"],\"series\":[{\"name\":\"\u200fA\",\"data\":[1]}]}", "series 1 的 name 不能包含双向文字控制字符"},
		"bidi unit":        {"{\"type\":\"bar\",\"unit\":\"\u061c%\",\"x\":[\"a\"],\"series\":[{\"data\":[1]}]}", "unit 不能包含双向文字控制字符"},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := ParseChart(tc.text); err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("got %v, want %q", err, tc.want)
			}
		})
	}
}

type promptCaptureModel struct{ system string }

func (m *promptCaptureModel) Stream(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	for _, message := range request.Messages {
		if message.Role == wire.RoleSystem {
			for _, block := range message.Content {
				m.system += block.Text
			}
		}
	}
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "ok"}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestChartInstructionsDescribeTheParsedFormat(t *testing.T) {
	if n := len(ChartInstructions); n < 400 || n > 800 {
		t.Fatal("model-facing chart description should stay near 600 bytes", n)
	}
	// The example in the prompt must itself parse: prompt and parser change together.
	start := strings.Index(ChartInstructions, "```chart\n")
	end := strings.Index(ChartInstructions[start+9:], "\n```")
	if start < 0 || end < 0 {
		t.Fatal("prompt lost its chart example")
	}
	if _, err := ParseChart(ChartInstructions[start+9 : start+9+end]); err != nil {
		t.Fatal("prompt example does not parse", err)
	}
	for _, word := range []string{"bar", "line", "area", "pie", "scatter", "null", "[[x,y],...]", "stacked", "unit", "title", "8 series", "100 bar/pie", "2000 line/area", "500 scatter", "80-byte", "32 KiB", "1e300"} {
		if !strings.Contains(ChartInstructions, word) {
			t.Fatal("prompt omits part of the format", word)
		}
	}
	model := &promptCaptureModel{}
	agent, err := core.NewBuilder(model, core.NewMemorySession("chart-prompt")).SystemPrompt("USER-SYSTEM").Plugin(ChartBlocksPlugin{}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "hi")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(model.system, "USER-SYSTEM\n\n") || !strings.HasSuffix(model.system, ChartInstructions) {
		t.Fatal("plugin did not append the chart description after the session system prompt", model.system)
	}
}
