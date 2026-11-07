// INPUT: Explicit document requests on platforms without the validated OS PDFKit worker.
// OUTPUT: Honest unsupported result with no decoder process or fallback parser.
// POS: [platform / OS primitive] Preserves the original file and never silently acquires dependencies.
//go:build !darwin

package document

import (
	"context"
	"os"
)

func (p *Processor) extract(context.Context, *os.File) (Output, error) {
	return Output{}, ErrUnsupported
}
