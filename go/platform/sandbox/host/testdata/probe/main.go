// INPUT: context, encoding/json, fmt, os, strings, runtime, unsafe, sandbox/abi, sandbox/guest
// OUTPUT: fixture main
// POS: [platform / WASM host or guest] Explicit local-only WASIp1 capability and resource attack fixture.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"strings"
	"unsafe"

	"github.com/QuincySx/Alva/go/platform/sandbox/guest"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

//go:wasmimport alva_host exchange
func exchange(operation, ptr, length uint32) uint32

func main() {
	config, err := guest.LoadContext()
	if err != nil {
		panic(err)
	}
	if len(os.Args) != 2 {
		panic("explicit probe mode required")
	}
	switch os.Args[1] {
	case "files":
		data, err := os.ReadFile("/work/in.txt")
		if err != nil {
			panic(err)
		}
		fmt.Printf("read=%s\n", data)
		if err := os.WriteFile("/work/out.txt", []byte("guest wrote"), 0600); err != nil {
			fmt.Println("write=denied")
		} else {
			fmt.Println("write=ok")
		}
		for _, name := range []string{"/ungranted/secret.txt", "/work/../secret.txt", "/work/escape", "/work/linkdir/secret.txt"} {
			if data, err := os.ReadFile(name); err == nil {
				panic("escaped: " + string(data))
			}
		}
		fmt.Println("escape=denied")
	case "environment":
		fmt.Print(strings.Join(os.Environ(), "\n"))
	case "spin":
		fmt.Println("spin entered")
		for {
		}
	case "memory":
		data := make([]byte, 512<<20)
		for i := range data {
			data[i] = byte(i)
		}
		fmt.Println(data[1])
	case "output":
		for {
			fmt.Print(strings.Repeat("x", 4096))
		}
	case "fetch":
		result, err := guest.Fetch(context.Background(), abi.FetchRequest{Version: abi.Version, Method: "GET", URL: config.Task})
		if err != nil {
			panic(err)
		}
		encoded, _ := json.Marshal(result)
		fmt.Print(string(encoded))
	case "escalate":
		result, err := guest.Escalate(context.Background(), abi.EscalationRequest{Version: abi.Version, Command: "explicit-command", Cwd: config.Task, TimeoutMS: 1000})
		if err != nil {
			panic(err)
		}
		encoded, _ := json.Marshal(result)
		fmt.Print(string(encoded))
	case "bad-abi":
		data := []byte(`{"version":99,"messages":[],"tools":[],"config":{}}`)
		exchange(abi.OpModel, uint32(uintptr(unsafe.Pointer(&data[0]))), uint32(len(data)))
		runtime.KeepAlive(data)
	default:
		panic("unknown probe")
	}
}
