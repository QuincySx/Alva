// INPUT: Complete original native tool arguments and explicit source/target protocols.
// OUTPUT: Private recursive numeric fidelity gate before float64-based third-party conversions.
// POS: [integration / protocol codec] Cross conversion rejects exact JSON numbers that Magpie could round instead of fabricating invocation parameters.
package codec

import (
	"bytes"
	"encoding/json"
	"io"
	"strconv"
	"strings"
)

func argumentNumberGuard(from, to Protocol, raw []byte) error {
	if from == to {
		return nil
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	for {
		token, err := decoder.Token()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return numberLoss(from, to)
		}
		if number, ok := token.(json.Number); ok {
			value, err := strconv.ParseFloat(string(number), 64)
			if err != nil {
				return numberLoss(from, to)
			}
			original, ok := normalizedDecimal(string(number))
			if !ok {
				return numberLoss(from, to)
			}
			roundTrip, ok := normalizedDecimal(strconv.FormatFloat(value, 'g', -1, 64))
			if !ok || original != roundTrip {
				return numberLoss(from, to)
			}
		}
	}
}
func numberLoss(from, to Protocol) error {
	return &LossError{Diagnostics: []Diagnostic{{Code: "tool_argument_precision_lost", Path: "tool.arguments", Message: "cross-protocol conversion cannot preserve an exact JSON numeric argument", Severity: Error, From: from, To: to}}}
}

// Compare decimal values without allocating exponent-sized big integers. The
// input already passed bounded strict JSON; an enormous exponent fails closed.
func normalizedDecimal(number string) (string, bool) {
	sign := ""
	if strings.HasPrefix(number, "-") {
		sign = "-"
		number = number[1:]
	}
	exponent := int64(0)
	if index := strings.IndexAny(number, "eE"); index >= 0 {
		parsed, err := strconv.ParseInt(number[index+1:], 10, 64)
		if err != nil || parsed > 10000 || parsed < -10000 {
			return "", false
		}
		exponent = parsed
		number = number[:index]
	}
	if dot := strings.IndexByte(number, '.'); dot >= 0 {
		exponent -= int64(len(number) - dot - 1)
		number = number[:dot] + number[dot+1:]
	}
	number = strings.TrimLeft(number, "0")
	if number == "" {
		return sign + "0", true
	}
	trimmed := strings.TrimRight(number, "0")
	exponent += int64(len(number) - len(trimmed))
	return sign + trimmed + "e" + strconv.FormatInt(exponent, 10), true
}
