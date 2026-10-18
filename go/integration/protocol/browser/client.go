// INPUT: context, encoding/json, errors, math, net/url, strings, sync, utf8 and pinned chromedp/cdproto.
// OUTPUT: Client, Connect, ValidateEndpoint, Tab, Action and bounded CDP navigation/status/interaction methods.
// POS: [integration / external protocol] Explicit CDP connection and typed page operations own neither browser processes nor product storage/permissions.
package browser

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"net"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/chromedp/cdproto/browser"
	"github.com/chromedp/cdproto/input"
	"github.com/chromedp/chromedp"
	"github.com/chromedp/chromedp/remote"
)

const MaxTabs = 16

type Tab struct {
	Index int    `json:"index"`
	URL   string `json:"url"`
	Title string `json:"title"`
}
type ownedTab struct {
	ctx    context.Context
	cancel context.CancelFunc
}
type Client struct {
	mu                      sync.Mutex
	ctx                     context.Context
	cancel, allocatorCancel context.CancelFunc
	tabs                    []ownedTab
	closed                  bool
}

// ValidateEndpoint accepts only the exact loopback browser WebSocket emitted
// by an explicitly owned process. It never probes /json/version or discovers
// existing Chrome instances, profiles or remote browser services.
func ValidateEndpoint(endpoint string) error {
	u, err := url.Parse(endpoint)
	if err != nil || len(endpoint) > 2048 || u.Scheme != "ws" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || !strings.HasPrefix(u.Path, "/devtools/browser/") || len(strings.TrimPrefix(u.Path, "/devtools/browser/")) < 1 {
		return errors.New("exact owned loopback browser WebSocket URL required")
	}
	ip := net.ParseIP(u.Hostname())
	port, err := strconv.Atoi(u.Port())
	if ip == nil || !ip.IsLoopback() || err != nil || port < 1 || port > 65535 || strings.ContainsAny(u.Path, "\x00\r\n") {
		return errors.New("browser debugger endpoint must be loopback with explicit port")
	}
	return nil
}

func Connect(ctx context.Context, endpoint string) (*Client, error) {
	if err := ValidateEndpoint(endpoint); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	// An operation deadline limits admission/initialization, not the retained
	// transport lifetime. This connection is always explicitly closed by owner.
	alloc, allocCancel := remote.NewAllocator(context.Background(), endpoint, remote.NoModifyURL)
	root, cancel := chromedp.NewContext(alloc)
	c := &Client{ctx: root, cancel: cancel, allocatorCancel: allocCancel, tabs: []ownedTab{{ctx: root, cancel: cancel}}}
	stop := context.AfterFunc(ctx, cancel)
	err := chromedp.Do(root, chromedp.Func(func(ctx context.Context, _ *chromedp.Target) error {
		_, err := chromedp.CallBrowser(ctx, browser.SetDownloadBehavior, browser.SetDownloadBehaviorParams{Behavior: browser.SetDownloadBehaviorBehaviorDeny})
		return err
	}))
	stop()
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		c.Close()
		return nil, err
	}
	return c, nil
}
func (c *Client) Close() {
	// Cancel before acquiring the operation lock: an action with no deadline
	// must not prevent Close from initiating transport/receiver cancellation.
	c.allocatorCancel()
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.closed {
		c.closed = true
		c.allocatorCancel()
		for _, tab := range c.tabs {
			tab.cancel()
		}
		c.cancel()
	}
}
func (c *Client) Done() <-chan struct{} { return c.ctx.Done() }
func (c *Client) active() (context.Context, error) {
	if c.closed || c.ctx.Err() != nil || len(c.tabs) == 0 {
		return nil, errors.New("browser connection closed")
	}
	select {
	case <-chromedp.FromContext(c.ctx).Browser.LostConnection:
		return nil, errors.New("browser connection lost")
	default:
	}
	return c.tabs[len(c.tabs)-1].ctx, nil
}
func operation(ctx, page context.Context) (context.Context, func()) {
	child, cancel := context.WithCancel(page)
	stop := context.AfterFunc(ctx, cancel)
	if ctx.Err() != nil {
		cancel()
	}
	return child, func() { stop(); cancel() }
}
func validURL(raw string) error {
	u, err := url.Parse(raw)
	if err != nil || len(raw) > 16<<10 || !utf8.ValidString(raw) || u.User != nil || (u.Scheme != "http" && u.Scheme != "https" && raw != "about:blank") || (raw != "about:blank" && u.Host == "") {
		return errors.New("explicit http/https URL or about:blank required; credentials/file/javascript URLs rejected")
	}
	return nil
}
func (c *Client) Navigate(ctx context.Context, raw string) (Tab, error) {
	if err := validURL(raw); err != nil {
		return Tab{}, err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if _, err := c.active(); err != nil {
		return Tab{}, err
	}
	if len(c.tabs) >= MaxTabs {
		return Tab{}, errors.New("browser tab budget exceeded")
	}
	page, cancel := chromedp.NewContext(c.ctx, chromedp.WithNewWindow(false))
	// First Run owns this tab's receiver lifetime. A temporary operation
	// context here would cancel its event receiver immediately after navigate
	// and leave subsequent DOM input/query operations permanently stalled.
	stop := context.AfterFunc(ctx, cancel)
	err := chromedp.Do(page, chromedp.Navigate(raw))
	stop()
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		cancel()
		return Tab{}, err
	}
	c.tabs = append(c.tabs, ownedTab{page, cancel})
	op, finish := operation(ctx, page)
	defer finish()
	return pageInfo(op, len(c.tabs)-1)
}
func pageInfo(ctx context.Context, index int) (Tab, error) {
	info, err := chromedp.Run(ctx, chromedp.Evaluate[Tab](`(()=>{const url=String(location.href),title=String(document.title);if(url.length>16384||title.length>4096)throw new Error("page metadata exceeds budget");return {url,title}})()`))
	info.Index = index
	return info, err
}
func (c *Client) Tabs(ctx context.Context) ([]Tab, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if _, err := c.active(); err != nil {
		return nil, err
	}
	infos := make([]Tab, 0, len(c.tabs))
	for index, page := range c.tabs {
		op, finish := operation(ctx, page.ctx)
		info, err := pageInfo(op, index)
		finish()
		if err != nil {
			return nil, err
		}
		infos = append(infos, info)
	}
	return infos, nil
}

type Action struct {
	Kind, Selector, Text, Key, Direction string
	X, Y                                 *float64
	Amount                               int64
}

func (c *Client) Action(ctx context.Context, a Action) error {
	if !utf8.ValidString(a.Selector) || len(a.Selector) > 2048 || !utf8.ValidString(a.Text) || len(a.Text) > 64<<10 {
		return errors.New("browser action text/selector exceeds budget")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	page, err := c.active()
	if err != nil {
		return err
	}
	op, finish := operation(ctx, page)
	defer finish()
	switch a.Kind {
	case "click":
		if a.Selector != "" {
			if a.X != nil || a.Y != nil {
				return errors.New("choose selector or coordinates, not both")
			}
			return chromedp.Do(op, chromedp.Click(chromedp.CSS(a.Selector)))
		}
		if a.X == nil || a.Y == nil || math.IsNaN(*a.X) || math.IsNaN(*a.Y) || math.IsInf(*a.X, 0) || math.IsInf(*a.Y, 0) || *a.X < 0 || *a.Y < 0 || *a.X > 16384 || *a.Y > 16384 {
			return errors.New("finite bounded click coordinates required")
		}
		return chromedp.Do(op, chromedp.MouseClickXY(*a.X, *a.Y))
	case "type":
		if a.Selector != "" {
			if err = chromedp.Do(op, chromedp.Click(chromedp.CSS(a.Selector))); err != nil {
				return err
			}
		}
		return chromedp.Do(op, chromedp.Func(func(ctx context.Context, t *chromedp.Target) error {
			_, err := chromedp.Call(ctx, input.InsertText, input.InsertTextParams{Text: a.Text})
			return err
		}))
	case "press":
		if a.Key == "" || len(a.Key) > 64 || !utf8.ValidString(a.Key) {
			return errors.New("bounded key name required")
		}
		return chromedp.Do(op, chromedp.Func(func(ctx context.Context, t *chromedp.Target) error {
			code := map[string]int64{"Enter": 13, "Tab": 9, "Escape": 27, "Backspace": 8, "Delete": 46, "ArrowLeft": 37, "ArrowUp": 38, "ArrowRight": 39, "ArrowDown": 40, "Home": 36, "End": 35, "PageUp": 33, "PageDown": 34, "Space": 32}[a.Key]
			if code == 0 {
				return errors.New("unsupported explicit key name")
			}
			for _, kind := range []input.DispatchKeyEventType{input.DispatchKeyEventTypeKeyDown, input.DispatchKeyEventTypeKeyUp} {
				_, err := chromedp.Call(ctx, input.DispatchKeyEvent, input.DispatchKeyEventParams{Type: kind, Key: a.Key, WindowsVirtualKeyCode: code})
				if err != nil {
					return err
				}
			}
			return nil
		}))
	case "scroll":
		if a.Amount == 0 {
			a.Amount = 300
		}
		if a.Amount < 1 || a.Amount > 10000 || (a.Direction != "" && a.Direction != "up" && a.Direction != "down") {
			return errors.New("invalid bounded scroll amount/direction")
		}
		if a.Direction == "up" {
			a.Amount = -a.Amount
		}
		data, _ := json.Marshal(a.Amount)
		_, err := chromedp.Run(op, chromedp.Evaluate[any](`window.scrollBy({top:`+string(data)+`,behavior:"instant"})`))
		return err
	default:
		return errors.New("unsupported browser action")
	}
}
