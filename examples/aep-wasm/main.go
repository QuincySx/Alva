// INPUT: WASIp1 stdin/stdout and the documented AEP v0.1 JSON-RPC protocol.
// OUTPUT: Standalone loadable plugin with hooks, reverse callbacks and an echo tool.
// POS: [external plugin example] Pure standard-library author code; never links the host or Agent runtime.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
)

type envelope struct {
	JSONRPC string                     `json:"jsonrpc"`
	ID      string                     `json:"id,omitempty"`
	Method  string                     `json:"method,omitempty"`
	Params  map[string]json.RawMessage `json:"params,omitempty"`
	Result  json.RawMessage            `json:"result,omitempty"`
	Error   json.RawMessage            `json:"error,omitempty"`
}

type server struct {
	scanner *bufio.Scanner
	output  *json.Encoder
	serial  int
	queued  []envelope
}

func (s *server) read() (envelope, bool) {
	if !s.scanner.Scan() {
		return envelope{}, false
	}
	var value envelope
	if json.Unmarshal(s.scanner.Bytes(), &value) != nil || value.JSONRPC != "2.0" {
		panic("invalid host frame")
	}
	return value, true
}
func (s *server) send(id string, result any) {
	if id != "" {
		if err := s.output.Encode(map[string]any{"jsonrpc": "2.0", "id": id, "result": result}); err != nil {
			panic("output failed")
		}
	}
}
func (s *server) call(method string, params any) json.RawMessage {
	s.serial++
	id := fmt.Sprintf("p-%d", s.serial)
	if err := s.output.Encode(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params}); err != nil {
		panic("callback write failed")
	}
	for {
		response, ok := s.read()
		if !ok {
			panic("callback input ended")
		}
		if response.Method == "" && response.ID == id {
			if len(response.Error) != 0 {
				panic("callback rejected")
			}
			return response.Result
		}
		// A notification may race a callback response. Preserve its order;
		// no goroutine blocks the single-threaded WASIp1 fd_read syscall.
		if response.Method == "" || len(s.queued) >= 128 {
			panic("unexpected callback response")
		}
		s.queued = append(s.queued, response)
	}
}
func text(text string, failed bool) any {
	return map[string]any{"content": []any{map[string]any{"type": "text", "text": text}}, "is_error": failed}
}
func msg(role, value string) any {
	return map[string]any{"role": role, "content": []any{map[string]any{"type": "text", "text": value}}}
}
func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 16<<20)
	s := server{scanner: scanner, output: json.NewEncoder(os.Stdout)}
	for {
		var request envelope
		if len(s.queued) > 0 {
			request = s.queued[0]
			s.queued = s.queued[1:]
		} else {
			var ok bool
			request, ok = s.read()
			if !ok {
				return
			}
		}
		result := any(map[string]any{"action": "continue"})
		switch request.Method {
		case "initialize":
			result = map[string]any{"protocolVersion": "0.1.0", "plugin": map[string]any{"name": "fixture", "version": "1"}, "tools": []any{map[string]any{"name": "echo", "description": "WASIp1 fixture echo", "inputSchema": map[string]any{"type": "object"}}}, "eventSubscriptions": []string{"on_llm_call_start", "before_tool_call", "after_tool_call", "on_llm_call_end"}, "requestedCapabilities": []string{"host:log", "host:state.get_metadata", "host:state.get_messages"}}
		case "initialized":
			continue
		case "shutdown":
			s.send(request.ID, map[string]any{})
			return
		case "extension/on_llm_call_start":
			var handle string
			_ = json.Unmarshal(request.Params["stateHandle"], &handle)
			metadata := s.call("host/state.get_metadata", map[string]any{"handle": handle})
			var view struct {
				Metadata struct {
					SessionID string `json:"session_id"`
				} `json:"metadata"`
			}
			if json.Unmarshal(metadata, &view) != nil || view.Metadata.SessionID == "" {
				panic("wrong authority")
			}
			s.call("host/state.get_messages", map[string]any{"handle": handle, "limit": 2})
			var messages []any
			_ = json.Unmarshal(request.Params["messages"], &messages)
			result = map[string]any{"action": "modify_messages", "messages": append([]any{msg("system", "sdk-system")}, messages...)}
		case "extension/before_tool_call":
			var call struct {
				Name string `json:"name"`
			}
			_ = json.Unmarshal(request.Params["toolCall"], &call)
			if call.Name == "target" {
				s.call("host/log", map[string]any{"level": "info", "message": "real WASM pre-intent callback"})
				result = map[string]any{"action": "modify", "modified_arguments": map[string]any{"path": "sdk-effective"}}
			}
		case "extension/after_tool_call":
			var call struct {
				Name string `json:"name"`
			}
			_ = json.Unmarshal(request.Params["toolCall"], &call)
			if call.Name == "target" {
				result = map[string]any{"action": "modify_result", "result": text("SDK transformed result", true)}
			}
		case "extension/on_llm_call_end":
			var response struct {
				Content []struct {
					Text string `json:"text"`
				} `json:"content"`
			}
			_ = json.Unmarshal(request.Params["response"], &response)
			for _, block := range response.Content {
				if block.Text == "done" {
					result = map[string]any{"action": "modify_response", "response": msg("assistant", "sdk-done")}
				}
			}
		case "tools/call":
			var args struct {
				Text  string `json:"text"`
				Hang  bool   `json:"hang"`
				Probe bool   `json:"probe"`
				Fail  bool   `json:"fail"`
			}
			_ = json.Unmarshal(request.Params["arguments"], &args)
			if args.Hang {
				s.call("host/log", map[string]any{"level": "info", "message": "fixture tool entered"})
				for {
				}
			}
			if args.Probe {
				_, err := os.ReadFile("/etc/passwd")
				result = text(fmt.Sprintf("env=%d;file_denied=%t", len(os.Environ()), err != nil), false)
			} else if args.Fail {
				result = text("explicit plugin failure", true)
			} else {
				result = text("WASM SDK echo: "+args.Text, false)
			}
		default:
			panic("unsupported host request")
		}
		if request.Method == "tools/call" {
			value := result.(map[string]any)
			value["isError"] = value["is_error"]
			delete(value, "is_error")
		}
		s.send(request.ID, result)
	}
}
