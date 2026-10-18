// INPUT: Wazero-validated Core V2 binary and an explicit bounded instruction budget
// OUTPUT: Versioned basic-block fuel instrumentation with fresh private per-instance counters
// POS: [platform / WASM host] No runtime fork, guest ABI change, time-based fuel substitute or host callback metering
package host

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

// The original binary MUST compile with the very same Core V2 validator
// before instrumentFuel is called. Appending globals must never make an
// originally invalid guest global index valid or grant access to the meter.
// The rewritten binary is validated again by wazero before instantiation.
const fuelRemainingExport = "__alva_fuel_v1_remaining"
const fuelExhaustedExport = "__alva_fuel_v1_exhausted"
const fuelInitializeExport = "__alva_fuel_v1_initialize"
const maxMeteredModule = 128 << 20
const MaxFuel uint64 = 1_000_000_000_000

var ErrFuelExhausted = errors.New("sandbox instruction fuel exhausted")

type wasmReader struct {
	data []byte
	pos  int
	err  error
}

func (r *wasmReader) byte() byte {
	if r.err != nil {
		return 0
	}
	if r.pos >= len(r.data) {
		r.err = errors.New("truncated WASM binary")
		return 0
	}
	v := r.data[r.pos]
	r.pos++
	return v
}
func (r *wasmReader) u32() uint32 {
	var value uint32
	for i := 0; i < 5; i++ {
		b := r.byte()
		if r.err != nil {
			return 0
		}
		if i == 4 && b > 15 {
			r.err = errors.New("invalid WASM u32 LEB")
			return 0
		}
		value |= uint32(b&127) << uint(i*7)
		if b&128 == 0 {
			return value
		}
	}
	r.err = errors.New("invalid WASM u32 LEB")
	return 0
}
func (r *wasmReader) signed(maximum int) {
	for i := 0; i < maximum; i++ {
		if r.byte()&128 == 0 || r.err != nil {
			return
		}
	}
	r.err = errors.New("invalid WASM signed LEB")
}
func (r *wasmReader) take(n uint32) []byte {
	if r.err != nil {
		return nil
	}
	if uint64(n) > uint64(len(r.data)-r.pos) {
		r.err = errors.New("truncated WASM section")
		return nil
	}
	v := r.data[r.pos : r.pos+int(n)]
	r.pos += int(n)
	return v
}
func (r *wasmReader) name() string { return string(r.take(r.u32())) }
func uleb(out []byte, n uint32) []byte {
	for {
		b := byte(n & 127)
		n >>= 7
		if n != 0 {
			b |= 128
		}
		out = append(out, b)
		if n == 0 {
			return out
		}
	}
}
func sleb(out []byte, n int64) []byte {
	for {
		b := byte(n & 127)
		n >>= 7
		last := n == 0 && b&64 == 0 || n == -1 && b&64 != 0
		if !last {
			b |= 128
		}
		out = append(out, b)
		if last {
			return out
		}
	}
}
func appendSection(out []byte, id byte, payload []byte) []byte {
	out = append(out, id)
	out = uleb(out, uint32(len(payload)))
	return append(out, payload...)
}

func importedGlobals(data []byte) (uint32, error) {
	r := wasmReader{data: data}
	count := r.u32()
	globals := uint32(0)
	limits := func() {
		flags := r.u32()
		if flags > 1 {
			r.err = errors.New("fuel supports Core V2 limits only")
			return
		}
		r.u32()
		if flags == 1 {
			r.u32()
		}
	}
	for i := uint32(0); i < count && r.err == nil; i++ {
		r.name()
		r.name()
		switch r.byte() {
		case 0:
			r.u32()
		case 1:
			r.byte()
			limits()
		case 2:
			limits()
		case 3:
			r.byte()
			r.byte()
			globals++
		default:
			r.err = errors.New("unsupported WASM import in fuel mode")
		}
	}
	if r.err == nil && r.pos != len(data) {
		r.err = errors.New("trailing WASM import bytes")
	}
	return globals, r.err
}

// These immediate widths are Core V2 only. Unknown proposals are refused,
// never guessed as one-byte operators. The unchanged original is separately
// validated by wazero, including reserved SIMD opcodes and lane/index bounds.
func fuelInstruction(r *wasmReader) (cost uint32, boundary bool) {
	op := r.byte()
	cost = 1
	switch {
	case op == 0x00 || op == 0x01 || op == 0x05 || op == 0x0b || op == 0x0f || op == 0x1a:
		cost = 0
		boundary = op != 0x01 && op != 0x1a
	case op >= 0x02 && op <= 0x04:
		r.signed(5)
		boundary = true
		if op != 0x04 {
			cost = 0
		}
	case op == 0x0c || op == 0x0d || op == 0x10:
		r.u32()
		boundary = true
	case op == 0x0e:
		n := r.u32()
		if uint64(n)+1 > uint64(len(r.data)-r.pos) {
			r.err = errors.New("invalid branch table")
			break
		}
		for i := uint32(0); i <= n && r.err == nil; i++ {
			r.u32()
		}
		boundary = true
	case op == 0x11:
		r.u32()
		r.u32()
		boundary = true
	case op == 0x1b:
	case op == 0x1c:
		n := r.u32()
		r.take(n)
	case op >= 0x20 && op <= 0x26 || op == 0xd2:
		r.u32()
	case op >= 0x28 && op <= 0x3e:
		r.u32()
		r.u32()
	case op == 0x3f || op == 0x40:
		r.u32()
	case op == 0x41:
		r.signed(5)
	case op == 0x42:
		r.signed(10)
	case op == 0x43:
		r.take(4)
	case op == 0x44:
		r.take(8)
	case op >= 0x45 && op <= 0xc4:
	case op == 0xd0:
		r.byte()
	case op == 0xd1:
	case op == 0xfc:
		sub := r.u32()
		switch sub {
		case 0, 1, 2, 3, 4, 5, 6, 7:
		case 8, 10, 12, 14:
			r.u32()
			r.u32()
		case 9, 11, 13, 15, 16, 17:
			r.u32()
		default:
			r.err = errors.New("unsupported extended WASM instruction in fuel mode")
		}
	case op == 0xfd:
		sub := r.u32()
		switch {
		case sub <= 11 || sub == 92 || sub == 93:
			r.u32()
			r.u32()
		case sub == 12 || sub == 13:
			r.take(16)
		case sub >= 21 && sub <= 34:
			r.byte()
		case sub >= 84 && sub <= 91:
			r.u32()
			r.u32()
			r.byte()
		case sub <= 255:
		default:
			r.err = errors.New("unsupported SIMD proposal in fuel mode")
		}
	default:
		r.err = fmt.Errorf("unsupported WASM opcode 0x%x in fuel mode", op)
	}
	return
}

// Charge a straight-line block before executing it. Calls/control transfers
// terminate blocks; every loop back-edge lands at a fresh charge. One unit
// per original operator except unreachable/nop/block/loop/else/end/return/drop
// (zero). Injected operators are uncharged. Bulk-memory byte length and trusted
// host callbacks are NOT instruction units; memory/IO/deadline remain separate.
func fuelCharge(out []byte, remaining, exhausted, cost uint32) []byte {
	if cost == 0 {
		return out
	}
	out = append(out, 0x23)
	out = uleb(out, remaining)
	out = append(out, 0x42)
	out = sleb(out, int64(cost))
	out = append(out, 0x54, 0x04, 0x40, 0x41, 0x01, 0x24)
	out = uleb(out, exhausted)
	out = append(out, 0x00, 0x0b, 0x23)
	out = uleb(out, remaining)
	out = append(out, 0x42)
	out = sleb(out, int64(cost))
	out = append(out, 0x7d, 0x24)
	return uleb(out, remaining)
}
func meterCode(ctx context.Context, data []byte, remaining uint32) ([]byte, error) {
	r := wasmReader{data: data}
	count := r.u32()
	out := uleb(nil, count)
	for i := uint32(0); i < count && r.err == nil; i++ {
		body := r.take(r.u32())
		b := wasmReader{data: body}
		locals := b.u32()
		for j := uint32(0); j < locals && b.err == nil; j++ {
			b.u32()
			b.byte()
		}
		if b.err != nil {
			return nil, b.err
		}
		metered := append([]byte(nil), body[:b.pos]...)
		start := b.pos
		cost := uint32(0)
		instructions := uint64(0)
		for b.pos < len(body) && b.err == nil {
			instructions++
			if instructions%256 == 0 {
				if err := ctx.Err(); err != nil {
					return nil, err
				}
			}
			units, boundary := fuelInstruction(&b)
			cost += units
			if boundary {
				metered = fuelCharge(metered, remaining, remaining+1, cost)
				metered = append(metered, body[start:b.pos]...)
				start = b.pos
				cost = 0
			}
			if len(metered) > maxMeteredModule {
				return nil, errors.New("metered function exceeds byte budget")
			}
		}
		if b.err != nil {
			return nil, b.err
		}
		if start != len(body) {
			return nil, errors.New("unterminated metered function")
		}
		out = uleb(out, uint32(len(metered)))
		out = append(out, metered...)
		if len(out) > maxMeteredModule {
			return nil, errors.New("metered code exceeds 128MiB")
		}
	}
	if r.err == nil && r.pos != len(data) {
		r.err = errors.New("trailing WASM code bytes")
	}
	return out, r.err
}

func instrumentFuel(ctx context.Context, data []byte, fuel uint64) ([]byte, error) {
	if fuel < 1 || fuel > MaxFuel || len(data) < 8 || len(data) > MaxModuleBytes {
		return nil, errors.New("invalid fuel instrumentation inputs")
	}
	type section struct {
		id   byte
		data []byte
	}
	sections := []section{}
	r := wasmReader{data: data, pos: 8}
	imported, defined := uint32(0), uint32(0)
	var initialize *uint32
	for r.pos < len(data) && r.err == nil {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		id := r.byte()
		payload := r.take(r.u32())
		sections = append(sections, section{id, payload})
		if id == 2 {
			var err error
			imported, err = importedGlobals(payload)
			if err != nil {
				return nil, err
			}
		}
		if id == 6 {
			v := wasmReader{data: payload}
			defined = v.u32()
			if v.err != nil {
				return nil, v.err
			}
		}
		if id == 8 {
			v := wasmReader{data: payload}
			index := v.u32()
			if v.err != nil || v.pos != len(payload) {
				return nil, errors.New("invalid original start section")
			}
			initialize = &index
		}
	}
	if r.err != nil {
		return nil, r.err
	}
	if uint64(imported)+uint64(defined)+2 > 1<<32-1 {
		return nil, errors.New("fuel global index overflow")
	}
	remaining := imported + defined
	globals := func(payload []byte) []byte {
		p := []byte(nil)
		if len(payload) > 0 {
			v := wasmReader{data: payload}
			v.u32()
			p = payload[v.pos:]
		}
		out := uleb(nil, defined+2)
		out = append(out, p...)
		out = append(out, 0x7e, 1, 0x42)
		out = sleb(out, int64(fuel))
		return append(out, 0x0b, 0x7f, 1, 0x41, 0, 0x0b)
	}
	exports := func(payload []byte) ([]byte, error) {
		v := wasmReader{data: payload}
		n := uint32(0)
		if len(payload) > 0 {
			n = v.u32()
		}
		start := v.pos
		for i := uint32(0); i < n && v.err == nil; i++ {
			name := v.name()
			if name == fuelRemainingExport || name == fuelExhaustedExport || name == fuelInitializeExport {
				return nil, errors.New("reserved fuel export collision")
			}
			v.byte()
			v.u32()
		}
		if v.err != nil {
			return nil, v.err
		}
		if v.pos != len(payload) {
			return nil, errors.New("trailing export bytes")
		}
		extra := uint32(2)
		if initialize != nil {
			extra++
		}
		out := uleb(nil, n+extra)
		out = append(out, payload[start:]...)
		for i, name := range []string{fuelRemainingExport, fuelExhaustedExport} {
			out = uleb(out, uint32(len(name)))
			out = append(out, name...)
			out = append(out, 3)
			out = uleb(out, remaining+uint32(i))
		}
		if initialize != nil {
			out = uleb(out, uint32(len(fuelInitializeExport)))
			out = append(out, fuelInitializeExport...)
			out = append(out, 0)
			out = uleb(out, *initialize)
		}
		return out, nil
	}
	out := append([]byte(nil), data[:8]...)
	hasGlobals, hasExports := false, false
	for _, s := range sections {
		if s.id != 0 && s.id > 6 && !hasGlobals {
			out = appendSection(out, 6, globals(nil))
			hasGlobals = true
		}
		if s.id != 0 && s.id > 7 && !hasExports {
			p, err := exports(nil)
			if err != nil {
				return nil, err
			}
			out = appendSection(out, 7, p)
			hasExports = true
		}
		payload := s.data
		switch s.id {
		case 8:
			// The host invokes the unchanged original start function before
			// _start, after memory/elements/globals are instantiated. Owning
			// the instance first also preserves fuel evidence on a start trap.
			continue
		case 0:
			v := wasmReader{data: payload}
			name := v.name()
			if v.err != nil {
				return nil, v.err
			}
			if strings.HasPrefix(name, ".debug_") || name == "sourceMappingURL" {
				continue
			}
		case 6:
			payload = globals(payload)
			hasGlobals = true
		case 7:
			var err error
			payload, err = exports(payload)
			if err != nil {
				return nil, err
			}
			hasExports = true
		case 10:
			var err error
			payload, err = meterCode(ctx, payload, remaining)
			if err != nil {
				return nil, err
			}
		}
		out = appendSection(out, s.id, payload)
		if len(out) > maxMeteredModule {
			return nil, errors.New("instrumented module exceeds 128MiB")
		}
	}
	if !hasGlobals {
		out = appendSection(out, 6, globals(nil))
	}
	if !hasExports {
		p, err := exports(nil)
		if err != nil {
			return nil, err
		}
		out = appendSection(out, 7, p)
	}
	return out, ctx.Err()
}
