// INPUT: encoding/json, unicode/utf8
// OUTPUT: ValidJSON and internal UTF-8 string validation
// POS: [L0 foundation] Reject byte-normalizing JSON inputs before encoding/json can replace malformed UTF-8.
package wire

import (
	"encoding/json"
	"unicode/utf8"
)

func ValidJSON(data json.RawMessage) bool {
	if !utf8.Valid(data) || !json.Valid(data) {
		return false
	}
	// encoding/json also replaces unpaired escaped UTF-16 surrogates. They are
	// byte-valid UTF-8 JSON but cannot round-trip to the same Unicode value.
	inString := false
	for i := 0; i < len(data); i++ {
		if !inString {
			if data[i] == '"' {
				inString = true
			}
			continue
		}
		if data[i] == '"' {
			inString = false
			continue
		}
		if data[i] != '\\' {
			continue
		}
		i++
		if data[i] != 'u' {
			continue
		}
		value := hexQuad(data[i+1 : i+5])
		i += 4
		if value >= 0xd800 && value <= 0xdbff {
			if i+6 >= len(data) || data[i+1] != '\\' || data[i+2] != 'u' {
				return false
			}
			low := hexQuad(data[i+3 : i+7])
			if low < 0xdc00 || low > 0xdfff {
				return false
			}
			i += 6
		} else if value >= 0xdc00 && value <= 0xdfff {
			return false
		}
	}
	return true
}
func hexQuad(raw []byte) uint16 {
	var value uint16
	for _, character := range raw {
		value *= 16
		switch {
		case character >= '0' && character <= '9':
			value += uint16(character - '0')
		case character >= 'a' && character <= 'f':
			value += uint16(character - 'a' + 10)
		case character >= 'A' && character <= 'F':
			value += uint16(character - 'A' + 10)
		}
	}
	return value
}
func validStrings(values ...string) bool {
	for _, value := range values {
		if !utf8.ValidString(value) {
			return false
		}
	}
	return true
}
