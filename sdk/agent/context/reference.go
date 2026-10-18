// INPUT: context, errors, unicode/utf8, llm/wire
// OUTPUT: ReferenceBackend, TextPage, TextMatch, ReferenceReader
// POS: [L3 capability library] Session-bound reference retrieval contracts and bounded text-reader capability without persistence policy.
package agentcontext

import (
	"context"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const MaxTextPageBytes = 4 << 10
const MaxTextMatches = 20

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
type ReferenceBackend interface {
	StoreText(context.Context, string) (wire.Reference, error)
	ReadText(context.Context, string, int64, int) (TextPage, error)
	SearchText(context.Context, string, string, int) ([]TextMatch, error)
}
type ReferenceReader struct{ Backend ReferenceBackend }

func (reader ReferenceReader) Read(ctx context.Context, uri string, offset int64, limit int) (TextPage, error) {
	if reader.Backend == nil {
		return TextPage{}, errors.New("reference backend unavailable")
	}
	if offset < 0 || limit < 1 || limit > MaxTextPageBytes {
		return TextPage{}, errors.New("invalid reference page bounds")
	}
	page, err := reader.Backend.ReadText(ctx, uri, offset, limit)
	if err != nil {
		return TextPage{}, err
	}
	if page.URI != uri || page.Offset != offset || page.NextOffset < offset || page.TotalBytes < page.NextOffset || len(page.Text) > limit || !utf8.ValidString(page.Text) || page.NextOffset != offset+int64(len(page.Text)) {
		return TextPage{}, errors.New("reference backend returned invalid page")
	}
	return page, nil
}
func (reader ReferenceReader) Search(ctx context.Context, uri, query string, limit int) ([]TextMatch, error) {
	if reader.Backend == nil {
		return nil, errors.New("reference backend unavailable")
	}
	if query == "" || len(query) > 1024 || limit < 1 || limit > MaxTextMatches {
		return nil, errors.New("invalid reference search bounds")
	}
	matches, err := reader.Backend.SearchText(ctx, uri, query, limit)
	if err != nil {
		return nil, err
	}
	if len(matches) > limit {
		return nil, errors.New("reference backend exceeded search result limit")
	}
	for _, match := range matches {
		if match.URI != uri || match.Offset < 0 || len(match.Text) > 256 || !utf8.ValidString(match.Text) {
			return nil, errors.New("reference backend returned invalid match")
		}
	}
	return append([]TextMatch(nil), matches...), nil
}
