// INPUT: github.com/tidwall/gjson.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	gjson "github.com/tidwall/gjson"
)

// emptyContent is a message's content with nothing in it: missing, "", or
// [] (or text blocks with no text).
func emptyContent(c gjson.Result) bool {
	switch {
	case !c.Exists() || c.Type == gjson.Null:
		return true
	case c.Type == gjson.String:
		return c.String() == ""
	case c.IsArray():
		empty := true
		c.ForEach(func(_, b gjson.Result) bool {
			empty = b.Get("type").String() == "text" && b.Get("text").String() == ""
			return empty
		})
		return empty
	}
	return false
}

// toolLoadedText is what a model is told of a tool ToolSearch loaded, where
// the tool_reference that names it can't go as it is.
func toolLoadedText(name string) string {
	return "Tool " + name + " is loaded and can be called now."
}
