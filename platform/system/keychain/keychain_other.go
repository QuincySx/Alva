// INPUT: An explicit keychain service name on a platform without an implementation
// OUTPUT: ErrUnsupported for every System vault request
// POS: [platform / OS primitive] Fail closed instead of storing secrets in a plaintext file
//go:build !darwin

package keychain

func system(string) (Vault, error) { return nil, ErrUnsupported }
