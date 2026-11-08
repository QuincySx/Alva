// INPUT: In-memory and (opt-in) real macOS keychain vaults
// OUTPUT: Put/Get/Exists/Delete round trips, bounds and missing-item regressions
// POS: [platform / OS primitive] The real keychain test writes one temporary item only with ALVA_KEYCHAIN_TEST=1
package keychain

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"runtime"
	"testing"
)

func exerciseVault(t *testing.T, vault Vault, account string) {
	t.Helper()
	ctx := context.Background()
	if ok, err := vault.Exists(ctx, account); err != nil || ok {
		t.Fatal("fresh account already exists", ok, err)
	}
	if _, err := vault.Get(ctx, account); !errors.Is(err, ErrNotFound) {
		t.Fatal("missing item did not report ErrNotFound", err)
	}
	secret := []byte(`{"access_token":"alpha","refresh_token":"beta"}`)
	if err := vault.Put(ctx, account, secret); err != nil {
		t.Fatal(err)
	}
	if ok, err := vault.Exists(ctx, account); err != nil || !ok {
		t.Fatal("stored item not found", ok, err)
	}
	if got, err := vault.Get(ctx, account); err != nil || !bytes.Equal(got, secret) {
		t.Fatal("round trip changed the secret", err)
	}
	replacement := []byte("rotated")
	if err := vault.Put(ctx, account, replacement); err != nil {
		t.Fatal(err)
	}
	if got, err := vault.Get(ctx, account); err != nil || !bytes.Equal(got, replacement) {
		t.Fatal("update did not replace the secret", err)
	}
	if err := vault.Delete(ctx, account); err != nil {
		t.Fatal(err)
	}
	if err := vault.Delete(ctx, account); err != nil {
		t.Fatal("deleting a missing item must be idempotent", err)
	}
	if ok, err := vault.Exists(ctx, account); err != nil || ok {
		t.Fatal("deleted item still exists", ok, err)
	}
	if err := vault.Put(ctx, "bad account", secret); err == nil {
		t.Fatal("account with spaces accepted")
	}
	if err := vault.Put(ctx, account, make([]byte, MaxSecret+1)); err == nil {
		t.Fatal("oversized secret accepted")
	}
}

func TestMemoryVault(t *testing.T) { exerciseVault(t, NewMemory(), "mcp-oauth:memory") }

func TestSystemVaultFailsClosedOffDarwin(t *testing.T) {
	if runtime.GOOS == "darwin" {
		t.Skip("darwin provides the system keychain")
	}
	if _, err := System("alva-test"); !errors.Is(err, ErrUnsupported) {
		t.Fatal("non-darwin platform did not refuse", err)
	}
}

// The real login keychain is touched only on explicit request.
func TestSystemVaultRoundTrip(t *testing.T) {
	if runtime.GOOS != "darwin" || os.Getenv("ALVA_KEYCHAIN_TEST") != "1" {
		t.Skip("set ALVA_KEYCHAIN_TEST=1 on macOS to exercise the login keychain")
	}
	var suffix [8]byte
	_, _ = rand.Read(suffix[:])
	vault, err := System("dev.alva.keychain-test")
	if err != nil {
		t.Fatal(err)
	}
	account := "test-" + hex.EncodeToString(suffix[:])
	t.Cleanup(func() { _ = vault.Delete(context.Background(), account) })
	exerciseVault(t, vault, account)
}
