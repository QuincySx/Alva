// INPUT: Explicit model defaults and portable codec requests in serializer regression fixtures.
// OUTPUT: Test-only request-default merger, without production compatibility APIs.
// POS: [integration / protocol codec] Keeps request/media configuration regressions beside their pure codec implementation.
package codec

func encodeChatFixture(defaults Request, request Request) ([]byte, error) {
	request.Model = defaults.Model
	request.MaxOutputTokens = defaults.MaxOutputTokens
	request.Stream = true
	return encodeChat(request)
}
func encodeNativeFixture(kind Protocol, defaults Request, request Request) (map[string]any, error) {
	request.Model = defaults.Model
	request.MaxOutputTokens = defaults.MaxOutputTokens
	request.Stream = true
	return encodeNative(kind, request)
}
