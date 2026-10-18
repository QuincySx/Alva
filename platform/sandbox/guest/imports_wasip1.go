// INPUT: runtime, sync, unsafe, sandbox/abi, alva_host wasm imports
// OUTPUT: exchange (private)
// POS: [platform / WASM host or guest] WASIp1-specific two-stage bounded host memory exchange without reentrant allocator exports.
//go:build wasip1 && wasm

package guest

import (
	"errors"
	"runtime"
	"sync"
	"unsafe"

	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

//go:wasmimport alva_host exchange
func hostExchange(operation, ptr, length uint32) uint32

//go:wasmimport alva_host read_response
func hostRead(ptr, length uint32) uint32

var importMu sync.Mutex

func fileToolsEnabled() bool { return true }

func exchange(operation uint32, data []byte, limit int) ([]byte, error) {
	importMu.Lock()
	defer importMu.Unlock()
	if len(data) == 0 || len(data) > abi.MaxModelRequestBytes {
		return nil, errors.New("invalid host request bytes")
	}
	size := hostExchange(operation, uint32(uintptr(unsafe.Pointer(&data[0]))), uint32(len(data)))
	runtime.KeepAlive(data)
	if size == 0 {
		if operation == abi.OpAudit {
			return nil, nil
		}
		return nil, errors.New("host omitted required response")
	}
	if size > uint32(limit) || size > abi.MaxResponseBytes {
		return nil, errors.New("host response byte limit exceeded")
	}
	response := make([]byte, int(size))
	actual := hostRead(uint32(uintptr(unsafe.Pointer(&response[0]))), size)
	runtime.KeepAlive(response)
	if actual != size {
		return nil, errors.New("host response length mismatch")
	}
	return response, nil
}
