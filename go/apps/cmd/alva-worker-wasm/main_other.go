// INPUT: fmt, os
// OUTPUT: main
// POS: [L6 application] Rejects native launch rather than silently running a supposedly sandboxed job outside WASI.
//go:build !wasip1 || !wasm

package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "alva-worker-wasm must be built for GOOS=wasip1 GOARCH=wasm and run by an explicit sandbox host")
	os.Exit(2)
}
