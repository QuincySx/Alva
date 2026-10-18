// INPUT: context, fmt, os, sandbox/guest
// OUTPUT: main
// POS: [L6 application] WASIp1 command executing the real shared SDK agent loop with explicit host imports.
//go:build wasip1 && wasm

package main

import (
	"context"
	"fmt"
	"github.com/QuincySx/Alva/go/platform/sandbox/guest"
	"os"
)

func main() {
	answer, err := guest.Run(context.Background())
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Print(answer)
}
