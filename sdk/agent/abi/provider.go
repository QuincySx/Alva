// INPUT: context, errors, fmt, reflect, sort, strings, sync, time, unicode/utf8, kernel/abi
// OUTPUT: Provider, ProviderRegistry, ProviderError, CredentialSource, StaticCredential
// POS: [L2.3 agent contracts] Explicit provider factories and credentials, with no network, ambient settings or transport implementation.
package abi

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	kernel "github.com/QuincySx/Alva/sdk/kernel/abi"
)

type ProviderErrorKind string

const (
	ProviderAPICall           ProviderErrorKind = "api_call"
	ProviderEmptyResponse     ProviderErrorKind = "empty_response"
	ProviderInvalidArgument   ProviderErrorKind = "invalid_argument"
	ProviderInvalidPrompt     ProviderErrorKind = "invalid_prompt"
	ProviderInvalidResponse   ProviderErrorKind = "invalid_response"
	ProviderJSONParse         ProviderErrorKind = "json_parse"
	ProviderCredential        ProviderErrorKind = "credential"
	ProviderSetting           ProviderErrorKind = "setting"
	ProviderNoContent         ProviderErrorKind = "no_content"
	ProviderNoSuchModel       ProviderErrorKind = "no_such_model"
	ProviderTooManyEmbeddings ProviderErrorKind = "too_many_embeddings"
	ProviderTypeValidation    ProviderErrorKind = "type_validation"
	ProviderUnsupported       ProviderErrorKind = "unsupported"
	ProviderNetwork           ProviderErrorKind = "network"
	ProviderRateLimited       ProviderErrorKind = "rate_limited"
)

// Sensitive response bodies are retained only as explicit evidence fields,
// never interpolated into Error(). Transport code owns redaction/bounds.
type ProviderError struct {
	Kind                                                     ProviderErrorKind
	Message, URL, ResponseBody, Argument, ModelID, ModelType string
	StatusCode                                               *int
	Retryable                                                bool
	RetryAfter                                               *time.Duration
	Count, Max                                               int
	Err                                                      error
}

func (err *ProviderError) Error() string {
	if err.Kind == ProviderNoSuchModel {
		return fmt.Sprintf("no such %s model: %s", err.ModelType, err.ModelID)
	}
	return string(err.Kind) + ": " + err.Message
}
func (err *ProviderError) Unwrap() error { return err.Err }

type Provider interface {
	ID() string
	LanguageModel(context.Context, string) (kernel.LanguageModel, error)
}

type ProviderRegistry struct {
	mu        sync.RWMutex
	providers map[string]Provider
}

func NewProviderRegistry() *ProviderRegistry {
	return &ProviderRegistry{providers: make(map[string]Provider)}
}

// Register explicitly replaces the same provider ID, matching factory registry
// semantics. This does not change the strict SDK Plugin/tool duplicate contract.
func (registry *ProviderRegistry) Register(provider Provider) error {
	if nilImplementation(provider) {
		return errors.New("provider required")
	}
	id := provider.ID()
	if id == "" || len(id) > 256 || !utf8.ValidString(id) {
		return errors.New("invalid provider identity")
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if registry.providers == nil {
		registry.providers = make(map[string]Provider)
	}
	if _, exists := registry.providers[id]; !exists && len(registry.providers) >= 256 {
		return errors.New("provider registry limit exceeded")
	}
	registry.providers[id] = provider
	return nil
}

func (registry *ProviderRegistry) Get(id string) (Provider, bool) {
	registry.mu.RLock()
	defer registry.mu.RUnlock()
	provider, exists := registry.providers[id]
	return provider, exists
}
func (registry *ProviderRegistry) IDs() []string {
	registry.mu.RLock()
	defer registry.mu.RUnlock()
	ids := make([]string, 0, len(registry.providers))
	for id := range registry.providers {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	return ids
}
func (registry *ProviderRegistry) LanguageModel(ctx context.Context, providerID, modelID string) (kernel.LanguageModel, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if modelID == "" || len(modelID) > 1024 || !utf8.ValidString(modelID) {
		return nil, &ProviderError{Kind: ProviderInvalidArgument, Argument: "model_id", Message: "invalid model identity"}
	}
	provider, exists := registry.Get(providerID)
	if !exists {
		return nil, &ProviderError{Kind: ProviderNoSuchModel, ModelID: providerID + ":" + modelID, ModelType: "language"}
	}
	model, err := provider.LanguageModel(ctx, modelID)
	if err == nil && (nilImplementation(model)) {
		return nil, &ProviderError{Kind: ProviderInvalidResponse, Message: "provider returned nil model"}
	}
	return model, err
}

type CredentialSource interface {
	GetAPIKey(context.Context) (string, error)
}
type StaticCredential struct{ key string }

func NewStaticCredential(key string) (*StaticCredential, error) {
	if key == "" || len(key) > 64<<10 || !utf8.ValidString(key) || strings.ContainsAny(key, "\r\n\x00") {
		return nil, &ProviderError{Kind: ProviderCredential, Message: "invalid bounded static credential"}
	}
	return &StaticCredential{key: key}, nil
}
func (credential StaticCredential) GetAPIKey(ctx context.Context) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if credential.key == "" {
		return "", &ProviderError{Kind: ProviderCredential, Message: "uninitialized static credential"}
	}
	return credential.key, nil
}
func (StaticCredential) String() string   { return "StaticCredential([REDACTED])" }
func (StaticCredential) GoString() string { return "StaticCredential([REDACTED])" }
func (StaticCredential) MarshalJSON() ([]byte, error) {
	return nil, errors.New("credential serialization forbidden")
}

func nilImplementation(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return v.IsNil()
	default:
		return false
	}
}
