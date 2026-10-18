// INPUT: encoding/json, errors, io, math and bounded pure message data
// OUTPUT: Module envelopes, Decode, Table and Chart validation
// POS: [L6 application] Versioned trusted-renderer data boundary distinct from executable Agent Plugins
package modules

import (
	"encoding/json"
	"errors"
	"io"
	"math"
	"strings"
)

type Module struct {
	Kind    string          `json:"kind"`
	Version int             `json:"version"`
	Data    json.RawMessage `json:"data"`
}

func Decode(source string) (Module, error) {
	var result Module
	if len(source) > 16<<10 {
		return result, errors.New("message module exceeds 16 KiB")
	}
	decoder := json.NewDecoder(strings.NewReader(source))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&result); err != nil {
		return result, err
	}
	var extra any
	if decoder.Decode(&extra) != io.EOF {
		return result, errors.New("message module has trailing data")
	}
	if result.Kind == "" || len(result.Kind) > 64 || result.Version < 1 || result.Version > 64 || !json.Valid(result.Data) {
		return result, errors.New("invalid message module envelope")
	}
	return result, nil
}

type Table struct {
	Columns []string   `json:"columns"`
	Rows    [][]string `json:"rows"`
}

func (t Table) Validate() error {
	if len(t.Columns) < 1 || len(t.Columns) > 12 || len(t.Rows) > 200 {
		return errors.New("table row/column budget exceeded")
	}
	for _, column := range t.Columns {
		if len(column) > 128 {
			return errors.New("table header too long")
		}
	}
	for _, row := range t.Rows {
		if len(row) != len(t.Columns) {
			return errors.New("table row shape mismatch")
		}
		for _, cell := range row {
			if len(cell) > 2048 {
				return errors.New("table cell too long")
			}
		}
	}
	return nil
}

type Chart struct {
	Labels []string  `json:"labels"`
	Values []float64 `json:"values"`
}

func (c Chart) Validate() error {
	if len(c.Labels) < 1 || len(c.Labels) > 64 || len(c.Labels) != len(c.Values) {
		return errors.New("chart shape exceeds limits")
	}
	for i, v := range c.Values {
		if math.IsNaN(v) || math.IsInf(v, 0) || v < 0 || v > 1e12 || len(c.Labels[i]) > 128 {
			return errors.New("chart value or label is invalid")
		}
	}
	return nil
}
