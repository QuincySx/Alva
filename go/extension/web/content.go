// INPUT: bytes, encoding/base64/json, errors, io, strings, unicode/utf8, x/net/html, storage/session.
// OUTPUT: Bounded HTML text conversion, known-credential source redaction and explicit Instant Answer parsing.
// POS: [L4 extension] Derived web text states its real processing and never implies media understanding or exhaustive search.
package web

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"golang.org/x/net/html"
)

func sanitize(data []byte) ([]byte, bool, error) {
	frame := map[string]string{}
	if utf8.Valid(data) {
		frame["text"] = string(data)
	} else {
		frame["base64"] = base64.StdEncoding.EncodeToString(data)
	}
	raw, _ := json.Marshal(frame)
	clean, err := session.RedactStreamFrame(raw)
	if err != nil {
		return nil, false, err
	}
	var decoded struct {
		Text     *string `json:"text"`
		Base64   *string `json:"base64"`
		Redacted bool    `json:"redacted"`
	}
	if err = json.Unmarshal(clean, &decoded); err != nil {
		return nil, false, err
	}
	if decoded.Text != nil {
		return []byte(*decoded.Text), decoded.Redacted, nil
	}
	if decoded.Base64 == nil {
		return nil, false, errors.New("invalid sanitized web frame")
	}
	value, err := base64.StdEncoding.DecodeString(*decoded.Base64)
	return value, decoded.Redacted, err
}

func htmlText(source []byte) ([]byte, error) {
	if !utf8.Valid(source) {
		return nil, errors.New("HTML text conversion requires UTF-8; original bytes remain reference-only")
	}
	tokenizer := html.NewTokenizer(bytes.NewReader(source))
	var output strings.Builder
	var skipped []string
	for count := 0; ; count++ {
		if count > 1_000_000 {
			return nil, errors.New("HTML token budget exceeded")
		}
		switch tokenizer.Next() {
		case html.ErrorToken:
			if tokenizer.Err() != io.EOF {
				return nil, tokenizer.Err()
			}
			return []byte(strings.TrimSpace(output.String())), nil
		case html.StartTagToken, html.SelfClosingTagToken:
			token := tokenizer.Token()
			if token.Data == "script" || token.Data == "style" || token.Data == "noscript" || token.Data == "template" {
				if len(skipped) >= 256 {
					return nil, errors.New("HTML skipped nesting exceeds 256")
				}
				skipped = append(skipped, token.Data)
			}
			if len(skipped) == 0 && blockTag(token.Data) {
				output.WriteByte('\n')
			}
		case html.EndTagToken:
			token := tokenizer.Token()
			if len(skipped) > 0 && token.Data == skipped[len(skipped)-1] {
				skipped = skipped[:len(skipped)-1]
			}
			if len(skipped) == 0 && blockTag(token.Data) {
				output.WriteByte('\n')
			}
		case html.TextToken:
			if len(skipped) == 0 {
				output.WriteString(strings.Join(strings.Fields(string(tokenizer.Text())), " "))
				output.WriteByte(' ')
			}
		}
		if output.Len() > 20<<20 {
			return nil, errors.New("derived HTML text exceeds 20 MiB")
		}
	}
}
func blockTag(name string) bool {
	switch name {
	case "p", "div", "br", "li", "ul", "ol", "table", "tr", "section", "article", "h1", "h2", "h3", "h4", "h5", "h6", "pre":
		return true
	}
	return false
}

type searchResult struct {
	Title string `json:"title"`
	URL   string `json:"url"`
	Text  string `json:"text"`
}

func instantAnswers(raw []byte, maximum int, allowed, blocked []string) ([]searchResult, error) {
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return nil, errors.New("search provider returned invalid JSON object")
	}
	if _, a := fields["AbstractText"]; !a {
		if _, b := fields["RelatedTopics"]; !b {
			return nil, errors.New("response is not an Instant Answer catalog")
		}
	}
	var data struct {
		Heading, AbstractText, AbstractURL, AbstractSource string
		RelatedTopics                                      []json.RawMessage
	}
	if err := json.Unmarshal(raw, &data); err != nil {
		return nil, err
	}
	var results []searchResult
	accept := func(title, address, text string) {
		if len(results) >= maximum || len(title) > 4096 || len(text) > 64<<10 {
			return
		}
		u, err := parseURL(address)
		if err != nil {
			return
		}
		host := strings.ToLower(u.Hostname())
		match := func(domains []string) bool {
			for _, d := range domains {
				if host == d || strings.HasSuffix(host, "."+d) {
					return true
				}
			}
			return false
		}
		if len(allowed) > 0 && !match(allowed) || match(blocked) {
			return
		}
		results = append(results, searchResult{Title: title, URL: address, Text: text})
	}
	if data.AbstractText != "" {
		accept(data.Heading, data.AbstractURL, data.AbstractText)
	}
	nodes := 0
	var walk func([]json.RawMessage, int) error
	walk = func(topics []json.RawMessage, depth int) error {
		if depth > 8 {
			return errors.New("search catalog nesting exceeds 8")
		}
		for _, item := range topics {
			nodes++
			if nodes > 1000 {
				return errors.New("search catalog exceeds 1000 entries")
			}
			var topic struct {
				Text, FirstURL string
				Topics         []json.RawMessage
			}
			if err := json.Unmarshal(item, &topic); err != nil {
				return err
			}
			if topic.FirstURL != "" {
				accept(topic.Text, topic.FirstURL, topic.Text)
			}
			if len(topic.Topics) > 0 {
				if err := walk(topic.Topics, depth+1); err != nil {
					return err
				}
			}
		}
		return nil
	}
	if err := walk(data.RelatedTopics, 0); err != nil {
		return nil, err
	}
	return results, nil
}
