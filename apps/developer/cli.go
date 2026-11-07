// INPUT: Explicit route JSON, optional bounded secrets stdin and a separate --run authorization.
// OUTPUT: Separate developer test/eval plans and bounded reports without raw model output or credentials.
// POS: [application developer tools] Product CLI frontend for model contracts and versioned task evaluation.
package developer

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type route struct {
	ID       string        `json:"id"`
	Protocol provider.Kind `json:"protocol"`
	Endpoint string        `json:"endpoint"`
	Model    string        `json:"model"`
}
type configuration struct {
	Version         int      `json:"version"`
	Dataset         *Dataset `json:"dataset,omitempty"`
	Routes          []route  `json:"routes"`
	ToolNames       []string `json:"tool_names"`
	Repeats         int      `json:"repeats"`
	MaxOutputTokens uint32   `json:"max_output_tokens"`
}
type Report struct {
	Version           int           `json:"version"`
	Kind              Mode          `json:"kind"`
	DatasetID         string        `json:"dataset_id,omitempty"`
	DatasetVersion    string        `json:"dataset_version,omitempty"`
	DatasetSHA256     string        `json:"dataset_sha256,omitempty"`
	Scores            []Score       `json:"scores,omitempty"`
	Mode              string        `json:"mode"`
	TotalPlannedCalls int           `json:"total_planned_calls"`
	Reports           []eval.Report `json:"reports"`
}

func readConfiguration(name string) (configuration, error) {
	var config configuration
	before, err := os.Lstat(name)
	if err != nil || !before.Mode().IsRegular() || before.Size() > 32<<10 {
		return config, errors.New("explicit regular evaluation config required; at most 32 KiB")
	}
	f, err := os.Open(name)
	if err != nil {
		return config, errors.New("evaluation config unavailable")
	}
	defer f.Close()
	after, err := f.Stat()
	if err != nil || !os.SameFile(before, after) {
		return config, errors.New("evaluation config changed while opening")
	}
	data, err := io.ReadAll(io.LimitReader(f, 32<<10+1))
	if err != nil || len(data) > 32<<10 {
		return config, errors.New("evaluation config exceeds limit")
	}
	if strictjson.ValidateObject(data, &config) != nil || json.Unmarshal(data, &config) != nil {
		return config, errors.New("invalid developer config: exact version/routes/tool_names/repeats/max_output_tokens/dataset fields required")
	}
	if config.Version != 1 || len(config.Routes) < 1 || len(config.Routes) > 8 {
		return config, errors.New("version 1 and 1..8 explicit routes required")
	}
	if config.MaxOutputTokens == 0 {
		config.MaxOutputTokens = 2048
	}
	if config.MaxOutputTokens > 16384 {
		return config, errors.New("max_output_tokens exceeds 16384")
	}
	if len(config.ToolNames) > 8 {
		return config, errors.New("at most 8 tool-name variants")
	}
	seen := map[string]bool{}
	for _, name := range config.ToolNames {
		if !portableToolName(name) || seen[name] {
			return config, errors.New("invalid or duplicate tool name")
		}
		seen[name] = true
	}
	return config, nil
}

func RunCLI(ctx context.Context, args []string, input io.Reader, stdout, stderr io.Writer) error {
	if len(args) == 0 || args[0] == "--help" || args[0] == "-h" {
		fmt.Fprintln(stdout, "alva developer test|eval --config FILE [--run] [--secrets-stdin]\n  test: model integration contracts; eval: versioned task quality and repeated-trial scores")
		return nil
	}
	mode := Mode(args[0])
	if mode != Test && mode != Evaluate {
		return errors.New("developer mode must be test or eval")
	}
	args = args[1:]
	flags := flag.NewFlagSet("alva developer "+string(mode), flag.ContinueOnError)
	flags.SetOutput(stderr)
	file := flags.String("config", "", "explicit versioned route JSON; default is a cold plan")
	execute := flags.Bool("run", false, "explicitly call configured models; may incur inference charges")
	secrets := flags.Bool("secrets-stdin", false, "read {route_id: api_key} from bounded pipe/regular stdin; never env or argv")
	maxCalls := flags.Int("max-calls", 32, "total planned model-call ceiling, 1..256; repeats are explicit trials, never retries")
	timeout := flags.Duration("per-call-timeout", 30*time.Second, "context deadline for each actual model request; at most 2m")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return errors.New("invalid developer flags")
	}
	if len(args) == 0 {
		flags.PrintDefaults()
		return nil
	}
	if flags.NArg() != 0 || *file == "" || *secrets && !*execute {
		return errors.New("explicit --config required; --secrets-stdin is only used with --run")
	}
	if *timeout <= 0 || *timeout > 2*time.Minute {
		return errors.New("per-call-timeout must be positive and at most 2m")
	}
	config, err := readConfiguration(*file)
	if err != nil {
		return err
	}
	if config.Repeats == 0 {
		config.Repeats = 1
		if mode == Evaluate {
			config.Repeats = 3
		}
	}
	cases, err := buildCases(mode, config.ToolNames, config.MaxOutputTokens, config.Dataset)
	if err != nil {
		return err
	}
	options := eval.Options{Repeats: config.Repeats, MaxCalls: *maxCalls, Timeout: *timeout}
	result := Report{Version: 1, Mode: "plan", Kind: mode, Reports: []eval.Report{}}
	ids := map[string]bool{}
	models := make([]abi.LanguageModel, len(config.Routes))
	for i, r := range config.Routes {
		if ids[r.ID] {
			return errors.New("duplicate route ID")
		}
		ids[r.ID] = true
		plan, e := eval.Plan(eval.Target{ID: r.ID, Protocol: string(r.Protocol), Model: r.Model}, cases, options)
		if e != nil {
			return e
		}
		result.TotalPlannedCalls += plan.PlannedCalls
		result.Reports = append(result.Reports, plan)
		models[i], e = provider.Build(r.Protocol, provider.Config{Endpoint: r.Endpoint, Model: r.Model, MaxOutputTokens: int(config.MaxOutputTokens)})
		if e != nil {
			return errors.New("invalid explicit provider route; no requests sent")
		}
	}
	if *maxCalls < 1 || *maxCalls > 256 || result.TotalPlannedCalls > *maxCalls {
		return errors.New("complete route matrix exceeds total --max-calls; no requests sent")
	}
	if !*execute {
		return json.NewEncoder(stdout).Encode(finalizeOutput(mode, config.Dataset, result))
	}
	if *secrets {
		data, e := secretinput.Read(ctx, input, 64<<10)
		if e != nil {
			return e
		}
		if strictjson.Validate(data) != nil {
			return errors.New("invalid credential object")
		}
		var raw map[string]json.RawMessage
		if json.Unmarshal(data, &raw) != nil || raw == nil || len(raw) > len(ids) {
			return errors.New("credential object must use configured route IDs")
		}
		keys := map[string]string{}
		for id, value := range raw {
			var key string
			if !ids[id] || bytes.Equal(bytes.TrimSpace(value), []byte("null")) || json.Unmarshal(value, &key) != nil {
				return errors.New("credential values must be strings for known route IDs")
			}
			keys[id] = key
		}
		for i, r := range config.Routes {
			models[i], err = provider.Build(r.Protocol, provider.Config{Endpoint: r.Endpoint, Model: r.Model, APIKey: keys[r.ID], MaxOutputTokens: int(config.MaxOutputTokens)})
			if err != nil {
				return errors.New("invalid explicit provider credentials; no requests sent")
			}
		}
	}
	result.Mode = "explicit-run"
	result.Reports = nil
	var runErr error
	failures, executionErrors := 0, 0
	for i, r := range config.Routes {
		report, e := eval.Run(ctx, models[i], eval.Target{ID: r.ID, Protocol: string(r.Protocol), Model: r.Model}, cases, options)
		result.Reports = append(result.Reports, report)
		failures += report.Failed + report.Errors
		executionErrors += report.Errors
		if e != nil {
			runErr = e
			break
		}
	}
	if err := json.NewEncoder(stdout).Encode(finalizeOutput(mode, config.Dataset, result)); err != nil {
		return err
	}
	if runErr != nil {
		return errors.New("evaluation interrupted; JSON contains partial results")
	}
	if executionErrors > 0 || mode == Test && failures > 0 {
		return errors.New("developer run contains failed checks or execution errors; inspect JSON results")
	}
	return nil
}
