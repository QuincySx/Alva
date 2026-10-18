// INPUT: strings.
// OUTPUT: Matches for bounded exact/prefix/suffix/contains lifecycle matching.
// POS: [L4 extension] Pure hook selection never treats user input as a regular expression or executable.
package hooks

import "strings"

func Matches(pattern, query string) bool {
	if pattern == "" || pattern == "*" {
		return true
	}
	if query == "" {
		return false
	}
	pattern, query = strings.ToLower(pattern), strings.ToLower(query)
	if strings.HasPrefix(pattern, "*") && strings.HasSuffix(pattern, "*") {
		return strings.Contains(query, strings.Trim(pattern, "*"))
	}
	if strings.HasPrefix(pattern, "*") {
		return strings.HasSuffix(query, strings.TrimPrefix(pattern, "*"))
	}
	if strings.HasSuffix(pattern, "*") {
		return strings.HasPrefix(query, strings.TrimSuffix(pattern, "*"))
	}
	return pattern == query
}
