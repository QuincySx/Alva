// INPUT: Pure hub scope, approval and remote MCP validation policy
// OUTPUT: ResolveSettings, CarryEnablement and transport validation regressions
// POS: [L7 Hub product] Application intent never runs without an exact workspace approval
package hub

import (
	"net/url"
	"testing"
)

func TestResolveSettingsNeedsExactWorkspaceApproval(t *testing.T) {
	app := AppSettings{Revision: 1, Capabilities: Capabilities{MCP: []MCPServer{{ID: "a", Command: "/bin/echo", Enabled: true}}, Tools: ToolSettings{Planning: true}}}
	item := app.Capabilities.Items()[0]
	session := Settings{Version: 1, SessionID: "s", MCP: []MCPServer{}}
	resolved := ResolveSettings(session, app, nil)
	if resolved.MCP[0].Enabled || resolved.MCP[0].HostAccessApproved || !resolved.Tools.Planning {
		t.Fatal("unapproved host item ran or plain switch lost", resolved.MCP)
	}
	approval := WorkspaceApproval{Workspace: "/w", Kind: CapabilityMCP, Item: "a", Fingerprint: item.Fingerprint}
	if resolved = ResolveSettings(session, app, []WorkspaceApproval{approval}); !resolved.MCP[0].Enabled || !resolved.MCP[0].HostAccessApproved || resolved.Validate() != nil {
		t.Fatal("exact approval did not apply", resolved.MCP)
	}
	app.Capabilities.MCP[0].Args = []string{"changed"}
	if resolved = ResolveSettings(session, app, []WorkspaceApproval{approval}); resolved.MCP[0].Enabled {
		t.Fatal("approval survived a configuration change")
	}
	app.Capabilities.MCP[0].Args, app.Capabilities.MCP[0].Name = nil, "renamed"
	if resolved = ResolveSettings(session, app, []WorkspaceApproval{approval}); !resolved.MCP[0].Enabled {
		t.Fatal("display rename invalidated the approval")
	}
	session.SubagentsEnabled = true
	if resolved = ResolveSettings(session, app, []WorkspaceApproval{approval}); len(resolved.MCP) != 0 || !resolved.SubagentsEnabled {
		t.Fatal("a session with its own configuration did not keep it")
	}
}

func TestApplicationSettingsRejectStoredApprovals(t *testing.T) {
	stored := AppSettings{Revision: 1, Capabilities: Capabilities{MCP: []MCPServer{{ID: "a", Command: "/bin/echo", Enabled: true, HostAccessApproved: true}}}}
	if stored.Validate() == nil {
		t.Fatal("application settings with an approval flag accepted")
	}
	stored.Capabilities = stored.Capabilities.Intent()
	if err := stored.Validate(); err != nil || stored.Capabilities.MCP[0].HostAccessApproved {
		t.Fatal("intent did not clear approvals", err)
	}
}

func TestCarryEnablementNeverEnablesAndRevokesChangedItems(t *testing.T) {
	current := Capabilities{MCP: []MCPServer{{ID: "a", Command: "/bin/echo", Enabled: true, HostAccessApproved: true}, {ID: "b", Command: "/bin/cat", Enabled: true, HostAccessApproved: true}}}
	next := current.Clone()
	next.MCP[1].Args = []string{"changed"}
	next.MCP = append(next.MCP, MCPServer{ID: "c", Command: "/bin/ls", Enabled: true, HostAccessApproved: true})
	carried := CarryEnablement(current, next, true)
	if !carried.MCP[0].Enabled || !carried.MCP[0].HostAccessApproved || carried.MCP[1].Enabled || carried.MCP[2].Enabled || carried.MCP[2].HostAccessApproved {
		t.Fatal("edit enabled a new item or kept a changed approval", carried.MCP)
	}
}

func TestRemoteMCPValidation(t *testing.T) {
	valid := func(server MCPServer) error {
		return Settings{Version: 1, SessionID: "s", MCP: []MCPServer{server}}.Validate()
	}
	withUser := (&url.URL{Scheme: "https", Host: "mcp.example.test", Path: "/mcp", User: url.UserPassword("someone", "pw")}).String()
	for name, server := range map[string]MCPServer{
		"lan http":       {ID: "r", Transport: MCPTransportHTTP, URL: "http://192.168.1.2/mcp"},
		"oauth https":    {ID: "r", Transport: MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Auth: MCPAuthOAuth},
		"oauth loopback": {ID: "r", Transport: MCPTransportHTTP, URL: "http://127.0.0.1:9/mcp", Auth: MCPAuthOAuth},
		"legacy stdio":   {ID: "r", Command: "npx"},
	} {
		if err := valid(server); err != nil {
			t.Fatal(name, err)
		}
	}
	for name, server := range map[string]MCPServer{
		"oauth over lan http": {ID: "r", Transport: MCPTransportHTTP, URL: "http://192.168.1.2/mcp", Auth: MCPAuthOAuth},
		"credentials in url":  {ID: "r", Transport: MCPTransportHTTP, URL: withUser},
		"remote with command": {ID: "r", Transport: MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Command: "/bin/sh"},
		"local with url":      {ID: "r", Command: "npx", URL: "https://mcp.example.test/mcp"},
		"session header":      {ID: "r", Transport: MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Headers: map[string]string{"Mcp-Session-Id": "x"}},
		"oauth plus header":   {ID: "r", Transport: MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Auth: MCPAuthOAuth, Headers: map[string]string{"Authorization": "Bearer x"}},
		"unknown transport":   {ID: "r", Transport: "sse", URL: "https://mcp.example.test/sse"},
		"header injection":    {ID: "r", Transport: MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Headers: map[string]string{"X-A": "v\r\nX-B: y"}},
	} {
		if valid(server) == nil {
			t.Fatal("invalid remote server accepted:", name)
		}
	}
}
