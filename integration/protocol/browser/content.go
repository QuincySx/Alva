// INPUT: bytes, context, encoding/json, errors, fmt, image, math, strings, utf8 and pinned chromedp/cdproto.
// OUTPUT: Snapshot, Screenshot, ScreenshotOptions and full bounded content/screenshot CDP methods.
// POS: [integration / external protocol] Source extraction and actual image encoding metadata never imply AI understanding, complete article semantics or model delivery.
package browser

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	_ "image/jpeg"
	_ "image/png"
	"math"
	"strings"
	"unicode/utf8"

	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/chromedp"
)

const MaxTextBytes = 4 << 20
const MaxImageBytes = 20 << 20

type Snapshot struct {
	Tab        Tab    `json:"tab"`
	Mode       string `json:"mode"`
	Content    string `json:"content"`
	Processing string `json:"processing"`
}

func (c *Client) Snapshot(ctx context.Context, mode, selector string) (Snapshot, error) {
	if mode == "" {
		mode = "text"
	}
	if (mode != "text" && mode != "html" && mode != "readability") || len(selector) > 2048 || !utf8.ValidString(selector) {
		return Snapshot{}, errors.New("invalid snapshot mode/selector")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	target, err := c.active()
	if err != nil {
		return Snapshot{}, err
	}
	op, finish := operation(ctx, target)
	defer finish()
	encoded, _ := json.Marshal(selector)
	expression := `(()=>{const selector=` + string(encoded) + `;const node=selector?document.querySelector(selector):document.documentElement;if(!node)throw new Error("selected element not found");let value="";`
	switch mode {
	case "text":
		expression += `value=selector?node.innerText:(document.body?document.body.innerText:"");`
	case "html":
		expression += `value=node.outerHTML;`
	case "readability":
		expression += `const clone=node.cloneNode(true);clone.querySelectorAll("script,style,nav,footer,header,aside,noscript,iframe").forEach(e=>e.remove());const noise=/sidebar|menu|footer|header|nav|banner|advert|popup|modal|cookie|social|share|comment/i;clone.querySelectorAll("[class],[id]").forEach(e=>{if(noise.test(String(e.className||""))||noise.test(e.id||""))e.remove()});const main=clone.querySelector("article,main,[role=main],.post-content,.article-content,.entry-content,#content,.content");value=String((main&&main.textContent.length>200?main:clone).textContent||"").trim();`
	}
	expression += `value=String(value||"");if(value.length>4194304||new TextEncoder().encode(value).length>4194304)throw new Error("snapshot exceeds 4 MiB; no successful truncation");return value})()`
	text, err := chromedp.Run(op, chromedp.Evaluate[string](expression))
	if err != nil {
		return Snapshot{}, err
	}
	if len(text) > MaxTextBytes || !utf8.ValidString(text) {
		return Snapshot{}, errors.New("snapshot response exceeds valid UTF-8 budget")
	}
	info, err := pageInfo(op, len(c.tabs)-1)
	if err != nil {
		return Snapshot{}, err
	}
	processing := "Complete bounded DOM innerText extraction, not OCR, semantic understanding or automatic model delivery"
	if mode == "html" {
		processing = "Complete bounded DOM outerHTML serialization, not original response bytes or automatic model delivery"
	}
	if mode == "readability" {
		processing = "Heuristic cloned-DOM article/textContent extraction with configured noise tags/patterns removed; not Mozilla Readability or guaranteed complete article; no automatic model delivery"
	}
	return Snapshot{Tab: info, Mode: mode, Content: text, Processing: processing}, nil
}

type ScreenshotOptions struct {
	Format   string
	Quality  int64
	FullPage bool
	Selector string
}
type Screenshot struct {
	Data          []byte
	Format        string
	Width, Height int
	Processing    string
}

func (c *Client) Screenshot(ctx context.Context, o ScreenshotOptions) (Screenshot, error) {
	if o.Format == "" {
		o.Format = "png"
	}
	if (o.Format != "png" && o.Format != "jpeg") || o.Quality < 0 || o.Quality > 100 || len(o.Selector) > 2048 || !utf8.ValidString(o.Selector) || (o.FullPage && o.Selector != "") {
		return Screenshot{}, errors.New("invalid screenshot format/quality/scope")
	}
	if o.Format == "png" && o.Quality != 0 {
		return Screenshot{}, errors.New("PNG does not consume JPEG quality")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	target, err := c.active()
	if err != nil {
		return Screenshot{}, err
	}
	op, finish := operation(ctx, target)
	defer finish()
	var clip page.Viewport
	scope := "viewport"
	if o.Selector != "" {
		scope = "element"
		selector, _ := json.Marshal(o.Selector)
		clip, err = chromedp.Run(op, chromedp.Evaluate[page.Viewport](`(()=>{const e=document.querySelector(`+string(selector)+`);if(!e)throw new Error("screenshot element not found");const r=e.getBoundingClientRect();return {x:r.left+scrollX,y:r.top+scrollY,width:r.width,height:r.height,scale:1}})()`))
	} else if o.FullPage {
		scope = "full_page"
		var metrics page.GetLayoutMetricsResult
		metrics, err = chromedp.Run(op, func(ctx context.Context, t *chromedp.Target) (page.GetLayoutMetricsResult, error) {
			return cdp.Call(ctx, t, page.GetLayoutMetrics, cdp.Empty{})
		})
		if err == nil && metrics.CSSContentSize == nil {
			return Screenshot{}, errors.New("CDP omitted full-page layout geometry")
		}
		if err != nil {
			return Screenshot{}, err
		}
		clip = page.Viewport{X: metrics.CSSContentSize.X, Y: metrics.CSSContentSize.Y, Width: metrics.CSSContentSize.Width, Height: metrics.CSSContentSize.Height, Scale: 1}
	} else {
		clip, err = chromedp.Run(op, chromedp.Evaluate[page.Viewport](`({x:scrollX,y:scrollY,width:innerWidth,height:innerHeight,scale:1})`))
	}
	if err != nil {
		return Screenshot{}, err
	}
	for _, v := range []float64{clip.X, clip.Y, clip.Width, clip.Height} {
		if math.IsNaN(v) || math.IsInf(v, 0) {
			return Screenshot{}, errors.New("invalid screenshot geometry")
		}
	}
	if clip.Width < 1 || clip.Height < 1 || clip.Width > 16384 || clip.Height > 16384 || clip.Width*clip.Height > 32<<20 || clip.X < 0 || clip.Y < 0 {
		return Screenshot{}, errors.New("screenshot geometry exceeds 32 Mi pixels/16384 dimension")
	}
	params := page.CaptureScreenshotParams{Format: page.CaptureScreenshotFormat(o.Format), Clip: &clip, CaptureBeyondViewport: ptr(true), FromSurface: ptr(true)}
	if o.Format == "jpeg" {
		params.Quality = &o.Quality
	}
	result, err := chromedp.Run(op, func(ctx context.Context, t *chromedp.Target) (page.CaptureScreenshotResult, error) {
		return cdp.Call(ctx, t, page.CaptureScreenshot, params)
	})
	if err != nil {
		return Screenshot{}, err
	}
	if len(result.Data) > MaxImageBytes {
		return Screenshot{}, errors.New("screenshot exceeds 20 MiB; not retained as successful prefix")
	}
	info, format, err := image.DecodeConfig(bytes.NewReader(result.Data))
	if err != nil || format != o.Format || info.Width < 1 || info.Height < 1 || info.Width > 16384 || info.Height > 16384 || int64(info.Width)*int64(info.Height) > 32<<20 {
		return Screenshot{}, errors.New("screenshot encoding/dimensions contradict capture budget")
	}
	processing := strings.Join([]string{"CDP Page.captureScreenshot", scope, o.Format, "actual encoded screenshot bytes; no OCR, semantic understanding or automatic model delivery"}, "; ")
	if o.Format == "jpeg" {
		processing += fmt.Sprintf("; JPEG quality=%d", o.Quality)
	}
	return Screenshot{Data: result.Data, Format: format, Width: info.Width, Height: info.Height, Processing: processing}, nil
}
func ptr[T any](value T) *T { return &value }
