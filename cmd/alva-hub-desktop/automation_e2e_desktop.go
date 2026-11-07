// INPUT: Explicit desktop&&e2e build and MyGo's opt-in private MYGO_AUTOMATION Unix socket
// OUTPUT: Upstream native UI automation plugin for built-app windows, tree, actions and captures
// POS: [L7 Hub product] Keeps upstream end-to-end inspection out of ordinary desktop builds; adds no product runtime or protocol
//go:build desktop && e2e

package main

import _ "github.com/egoist/mygo/plugins/automation"
