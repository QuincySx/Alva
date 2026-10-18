// INPUT: encoding/json, errors, fmt, reflect, kernel/abi
// OUTPUT: Native transaction record-count and encoded/raw-input admission
// POS: [Native storage component] Resource policy stays outside the stable kernel contracts and generic SDK memory backend.
package session

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"reflect"
)

const maxBatchInputBytes = 64 << 20

// validateRecordBatchBudget checks shared count/encoded-and-raw-byte limits before
// a backend deep-copies or transforms a batch. Union, identity, causal and
// protocol validation remain required separately, within the atomic append.
func validateRecordBatchBudget(records []abi.SessionRecord) error {
	if len(records) > MaxBatchRecords {
		return fmt.Errorf("record batch exceeds %d records", MaxBatchRecords)
	}
	remaining := maxBatchInputBytes
	for _, record := range records {
		rawRemaining := remaining
		if !boundedRecordInput(reflect.ValueOf(record), &rawRemaining, 0) {
			return fmt.Errorf("record batch exceeds %d input bytes or structural depth", maxBatchInputBytes)
		}
		encoded, err := json.Marshal(record)
		if err != nil {
			return errors.New("record batch JSON encoding failed")
		}
		cost := max(len(encoded), remaining-rawRemaining)
		if cost > remaining {
			return fmt.Errorf("record batch exceeds %d serialized bytes", maxBatchInputBytes)
		}
		remaining -= cost
	}
	return nil
}

// JSON encoding compacts RawMessage whitespace. Count the original string/byte
// buffers too, so cheap encoded JSON cannot authorize an oversized deep copy.
// A depth cap also rejects cyclic in-process values before JSON marshaling.
func boundedRecordInput(value reflect.Value, remaining *int, depth int) bool {
	if depth > 64 {
		return false
	}
	switch value.Kind() {
	case reflect.Pointer, reflect.Interface:
		return value.IsNil() || boundedRecordInput(value.Elem(), remaining, depth+1)
	case reflect.String:
		if value.Len() > *remaining {
			return false
		}
		*remaining -= value.Len()
	case reflect.Slice:
		if value.Type().Elem().Kind() == reflect.Uint8 {
			if value.Len() > *remaining {
				return false
			}
			*remaining -= value.Len()
			return true
		}
		fallthrough
	case reflect.Array:
		for i := 0; i < value.Len(); i++ {
			if !boundedRecordInput(value.Index(i), remaining, depth+1) {
				return false
			}
		}
	case reflect.Struct:
		for i := 0; i < value.NumField(); i++ {
			if value.Type().Field(i).PkgPath == "" && !boundedRecordInput(value.Field(i), remaining, depth+1) {
				return false
			}
		}
	case reflect.Map:
		entries := value.MapRange()
		for entries.Next() {
			if !boundedRecordInput(entries.Key(), remaining, depth+1) || !boundedRecordInput(entries.Value(), remaining, depth+1) {
				return false
			}
		}
	}
	return true
}
