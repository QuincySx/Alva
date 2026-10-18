// INPUT: encoding/json, strings, testing, kernel/abi
// OUTPUT: Native maximum-batch acceptance and atomic count/encoded/raw-byte rejection tests
// POS: [Native storage testing] Backend resource policies remain separate from the generic SDK session contract.
package session

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

func TestNativeBatchMaximumAndAtomicCountRejection(t *testing.T) {
	repository, log := repoSession(t)
	record := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "example.budget", Type: "capacity", SchemaVersion: 1, Payload: json.RawMessage(`{}`)}
	batch := make([]abi.SessionRecord, MaxBatchRecords)
	for i := range batch {
		batch[i] = record
	}
	committed, err := log.AppendRecords(t.Context(), batch)
	if err != nil || len(committed) != MaxBatchRecords {
		t.Fatal("native maximum batch rejected", err)
	}
	if _, err = log.AppendRecords(t.Context(), append(batch, record)); err == nil {
		t.Fatal("native oversized batch accepted")
	}
	page, err := repository.Page(t.Context(), "owner", log.ID(), 0, 1, 0)
	if err != nil || page.HighWater != uint64(MaxBatchRecords) {
		t.Fatal("oversized batch partly committed", page.HighWater, err)
	}
	last := committed[len(committed)-1]
	tail, err := log.Records(t.Context(), last.Sequence-1)
	if err != nil || len(tail) != 1 || tail[0].ID != last.ID {
		t.Fatal("maximum batch receipt disagrees with storage", err)
	}
}

func TestNativeBatchByteBudgetsRejectAtomically(t *testing.T) {
	for _, rawWhitespace := range []bool{false, true} {
		t.Run(map[bool]string{false: "encoded", true: "raw-whitespace"}[rawWhitespace], func(t *testing.T) {
			_, log := repoSession(t)
			payload := json.RawMessage(`{"text":"` + strings.Repeat("x", 1<<20) + `"}`)
			if rawWhitespace {
				payload = json.RawMessage(strings.Repeat(" ", 1<<20) + `{}`)
			}
			batch := make([]abi.SessionRecord, 65)
			for i := range batch {
				batch[i] = abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "example.budget", Type: "bytes", SchemaVersion: 1, Payload: payload}
			}
			if _, err := log.AppendRecords(t.Context(), batch); err == nil {
				t.Fatal("native byte budget bypassed")
			}
			stored, err := log.Records(t.Context(), 0)
			if err != nil || len(stored) != 0 {
				t.Fatal("byte rejection partly committed", err)
			}
		})
	}
}
