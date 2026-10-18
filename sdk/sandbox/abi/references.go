// INPUT: errors, llm/wire
// OUTPUT: ReferenceRead/Search, TextPage/Match, ReferenceResult, HistoryRequest/Entry/Page
// POS: [L0 foundation] Read-only, session-bound reference and historical index DTOs without database or host-path authority.
package abi

import (
	"errors"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type ReferenceRead struct {
	Version uint32 `json:"version"`
	URI     string `json:"uri"`
	Offset  int64  `json:"offset"`
	Limit   int    `json:"limit"`
}

func (v ReferenceRead) Validate() error {
	if v.Version != Version || !Identity(v.URI) || v.Offset < 0 || v.Limit < 1 || v.Limit > 4096 {
		return errors.New("invalid reference page bounds")
	}
	return nil
}

type ReferenceSearch struct {
	Version uint32 `json:"version"`
	URI     string `json:"uri"`
	Query   string `json:"query"`
	Limit   int    `json:"limit"`
}

func (v ReferenceSearch) Validate() error {
	if v.Version != Version || !Identity(v.URI) || v.Query == "" || !Text(v.Query) || len(v.Query) > 256 || v.Limit < 1 || v.Limit > 20 {
		return errors.New("invalid literal reference search")
	}
	return nil
}

type TextPage struct {
	URI        string `json:"uri"`
	Text       string `json:"text"`
	Offset     int64  `json:"offset"`
	NextOffset int64  `json:"next_offset"`
	TotalBytes int64  `json:"total_bytes"`
	Truncated  bool   `json:"truncated"`
}
type TextMatch struct {
	URI    string `json:"uri"`
	Text   string `json:"text"`
	Offset int64  `json:"offset"`
}
type ReferenceResult struct {
	Version uint32      `json:"version"`
	Page    *TextPage   `json:"page,omitempty"`
	Matches []TextMatch `json:"matches,omitempty"`
}

func (v ReferenceResult) ValidateRead(r ReferenceRead) error {
	if v.Version != Version || v.Page == nil || v.Matches != nil {
		return errors.New("invalid reference read result")
	}
	p := v.Page
	if p.URI != r.URI || p.Offset != r.Offset || !Text(p.Text) || len(p.Text) > r.Limit || p.NextOffset < p.Offset || p.NextOffset != p.Offset+int64(len(p.Text)) || p.TotalBytes < p.NextOffset || p.Truncated != (p.NextOffset < p.TotalBytes) {
		return errors.New("invalid reference text page")
	}
	return nil
}
func (v ReferenceResult) ValidateSearch(r ReferenceSearch) error {
	if v.Version != Version || v.Page != nil || len(v.Matches) > r.Limit {
		return errors.New("invalid reference search result")
	}
	for _, m := range v.Matches {
		if m.URI != r.URI || m.Offset < 0 || !Text(m.Text) || len(m.Text) > 256 {
			return errors.New("invalid reference match")
		}
	}
	return nil
}

type HistoryRequest struct {
	Version   uint32 `json:"version"`
	Query     string `json:"query,omitempty"`
	After     uint64 `json:"after"`
	HighWater uint64 `json:"high_water"`
	Limit     int    `json:"limit"`
}

func (v HistoryRequest) Validate() error {
	if v.Version != Version || !Text(v.Query) || len(v.Query) > 256 || v.Limit < 1 || v.Limit > 20 || (v.HighWater > 0 && v.After > v.HighWater) {
		return errors.New("invalid history index bounds")
	}
	return nil
}

type HistoryEntry struct {
	Sequence  uint64         `json:"sequence"`
	Namespace string         `json:"namespace"`
	Type      string         `json:"type"`
	Summary   string         `json:"summary"`
	Value     wire.Reference `json:"value"`
	Snippet   string         `json:"snippet,omitempty"`
}
type HistoryPage struct {
	Version   uint32         `json:"version"`
	Entries   []HistoryEntry `json:"entries"`
	Next      uint64         `json:"next"`
	HighWater uint64         `json:"high_water"`
	HasMore   bool           `json:"has_more"`
}

func (v HistoryPage) Validate(r HistoryRequest) error {
	if v.Version != Version || len(v.Entries) > r.Limit || v.Next < r.After || v.Next > v.HighWater || v.HasMore != (v.Next < v.HighWater) || (r.HighWater > 0 && v.HighWater != r.HighWater) {
		return errors.New("invalid history index result")
	}
	seq := r.After
	for _, e := range v.Entries {
		if e.Sequence <= seq || e.Sequence > v.Next || !Identity(e.Namespace) || !Identity(e.Type) || !Text(e.Summary) || !Text(e.Snippet) || len(e.Summary) > 1024 || len(e.Snippet) > 256 {
			return errors.New("invalid history index entry")
		}
		if err := (wire.ContentBlock{Kind: wire.ContentReference, Reference: &e.Value}).Validate(); err != nil {
			return err
		}
		seq = e.Sequence
	}
	return nil
}
