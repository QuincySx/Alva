// INPUT: Captured private originals, bounded ZIP/XML readers, optional owned PDFKit processor and immutable file metadata.
// OUTPUT: Honest format processing receipts and at most 2 MiB of inert Office/OpenDocument/PDF text.
// POS: [L7 Hub product] Extracts supported document text without running macros, embedded content, scripts or user applications.
package product

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
)

const maxComposerArchiveBytes = 32 << 20
const maxComposerXMLBytes = 8 << 20
const maxComposerArchiveEntries = 4096

type composerTextBuilder struct{ strings.Builder }

func (b *composerTextBuilder) add(text string) error {
	if !utf8.ValidString(text) || b.Len()+len(text) > int(hub.MaxComposerExtractedBytes) {
		return errors.New("extracted text exceeds the 2 MiB processing limit")
	}
	b.WriteString(text)
	return nil
}

func (s *HubService) extractComposerDocument(ctx context.Context, directory string, f *hub.ComposerFile) {
	ext := strings.ToLower(filepath.Ext(f.Name))
	if f.MediaType == "application/pdf" {
		ext = ".pdf"
	}
	var format string
	switch ext {
	case ".docx":
		format = "Word DOCX"
	case ".pptx":
		format = "PowerPoint PPTX"
	case ".xlsx":
		format = "Excel XLSX"
	case ".odt", ".odp", ".ods":
		format = "OpenDocument"
	case ".pdf":
		f.MediaType = "application/pdf"
		s.extractComposerPDF(ctx, directory, f)
		return
	case ".doc", ".xls", ".ppt", ".rtf":
		f.Processing += " Legacy/RTF document original retained; legacy binary and RTF extraction is unsupported by the built-in bounded parser. Use an approved document Skill/tool."
		return
	default:
		f.Processing += " Binary/media original retained; no decoder, OCR, transcription, archive unpacking or media understanding performed. Use an approved format-specific Skill/tool."
		return
	}
	text, err := extractComposerOffice(ctx, filepath.Join(directory, f.OriginalKey), f.Bytes, ext)
	if err != nil {
		f.Processing += " " + format + " original retained; text extraction unavailable: " + err.Error() + ". No macros or embedded content executed."
		return
	}
	saveComposerTextDerivative(directory, f, text, " "+format+" text extracted with bounded ZIP/XML parsing (2 MiB output, 8 MiB XML entry, 4096 archive entries). Text is a derivative, not a faithful layout/table/formula rendering; images, drawings, embedded files, macros and OCR are not processed.")
}

func saveComposerTextDerivative(directory string, f *hub.ComposerFile, text, processing string) {
	if int64(len(text)) > hub.MaxComposerExtractedBytes || !utf8.ValidString(text) {
		f.Processing += " Extracted text was not saved because its UTF-8/output budget was invalid."
		return
	}
	output, err := private.OpenFile(filepath.Join(directory, "extracted.txt"))
	if err == nil {
		_, err = io.WriteString(output, text)
		err = errors.Join(err, output.Sync(), output.Close())
	}
	if err != nil {
		f.Processing += " Original retained; the private extracted-text derivative could not be saved."
		return
	}
	digest := sha256.Sum256([]byte(text))
	f.TextKey = "extracted.txt"
	f.TextBytes = int64(len(text))
	f.TextSHA256 = hex.EncodeToString(digest[:])
	for start := 0; start < len(text); start += hub.ComposerFileChunkBytes {
		part := text[start:min(len(text), start+hub.ComposerFileChunkBytes)]
		digest := sha256.Sum256([]byte(part))
		f.TextChunkSHA256 = append(f.TextChunkSHA256, hex.EncodeToString(digest[:]))
	}
	f.Processing += processing
}

func (s *HubService) extractComposerPDF(ctx context.Context, directory string, f *hub.ComposerFile) {
	if s.Documents == nil {
		f.Processing += " PDF original retained; no trusted PDF extractor is configured. No text extraction, OCR, page rendering or document understanding performed. Use an approved PDF Skill/tool for this capture."
		return
	}
	source, err := os.Open(filepath.Join(directory, f.OriginalKey))
	if err != nil {
		f.Processing += " PDF original retained; its captured source could not be opened for text extraction."
		return
	}
	result, err := s.Documents.ExtractPDF(ctx, source)
	err = errors.Join(err, source.Close())
	if err != nil {
		f.Processing += " PDF original retained; native text extraction unavailable: " + err.Error() + ". No OCR, image decoding, document actions or page rendering performed."
		return
	}
	saveComposerTextDerivative(directory, f, result.Text, fmt.Sprintf(" PDF text extracted from %d pages using %s; text-only derivative, no OCR, page rendering, images, layout/table fidelity or document actions. Processing limits: 32 MiB input, 512 pages, 2 MiB UTF-8 output, 30 seconds and 256 MiB observed process memory checked every 20 ms (watchdog, not a kernel hard memory cap).", result.Pages, result.Processor))
}

// A small EOCD preflight rejects ZIP64, multi-disk and enormous entry indexes
// before archive/zip allocates its central-directory slice.
func composerZip(ctx context.Context, path string, size int64) (*zip.ReadCloser, error) {
	if size < 22 || size > maxComposerArchiveBytes {
		return nil, errors.New("document ZIP processing requires 22 bytes..32 MiB; original remains available")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, errors.New("captured original cannot be opened for document extraction")
	}
	defer file.Close()
	tail := make([]byte, min(int64(22+65535), size))
	if _, err = file.ReadAt(tail, size-int64(len(tail))); err != nil {
		return nil, err
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	found := false
	for i := len(tail) - 22; i >= 0; i-- {
		if string(tail[i:i+4]) != "PK\x05\x06" {
			continue
		}
		comment := int(binary.LittleEndian.Uint16(tail[i+20:]))
		if i+22+comment != len(tail) {
			continue
		}
		entries := binary.LittleEndian.Uint16(tail[i+10:])
		if binary.LittleEndian.Uint16(tail[i+4:]) != 0 || binary.LittleEndian.Uint16(tail[i+6:]) != 0 || binary.LittleEndian.Uint16(tail[i+8:]) != entries || entries == 0xffff || entries > maxComposerArchiveEntries || binary.LittleEndian.Uint32(tail[i+12:]) > 2<<20 {
			return nil, errors.New("ZIP64, multi-disk or more than 4096 archive entries are unsupported")
		}
		found = true
		break
	}
	if !found {
		return nil, errors.New("invalid bounded document ZIP directory")
	}
	reader, err := zip.OpenReader(path)
	if err != nil {
		var pathError *os.PathError
		if errors.As(err, &pathError) {
			return nil, errors.New("captured document archive cannot be opened")
		}
		return nil, err
	}
	if len(reader.File) > maxComposerArchiveEntries {
		reader.Close()
		return nil, errors.New("too many document ZIP entries")
	}
	return reader, nil
}
func composerXML(ctx context.Context, entry *zip.File, visit func(xml.Token) error) error {
	if entry.UncompressedSize64 > maxComposerXMLBytes {
		return errors.New("document XML entry exceeds 8 MiB")
	}
	reader, err := entry.Open()
	if err != nil {
		return err
	}
	defer reader.Close()
	bounded := &io.LimitedReader{R: reader, N: maxComposerXMLBytes + 1}
	decoder := xml.NewDecoder(bounded)
	for {
		if err = ctx.Err(); err != nil {
			return err
		}
		token, err := decoder.Token()
		if errors.Is(err, io.EOF) {
			if bounded.N <= 0 {
				return errors.New("document XML entry exceeds 8 MiB")
			}
			return nil
		}
		if err != nil {
			return err
		}
		if directive, ok := token.(xml.Directive); ok && strings.Contains(strings.ToUpper(string(directive)), "ENTITY") {
			return errors.New("document XML entities are unsupported")
		}
		if err = visit(token); err != nil {
			return err
		}
	}
}
func extractComposerOffice(ctx context.Context, path string, size int64, ext string) (string, error) {
	archive, err := composerZip(ctx, path, size)
	if err != nil {
		return "", err
	}
	defer archive.Close()
	var entries []*zip.File
	var shared *zip.File
	for _, entry := range archive.File {
		name := entry.Name
		if name == "xl/sharedStrings.xml" {
			shared = entry
		}
		match := ext == ".docx" && name == "word/document.xml" || ext == ".pptx" && strings.HasPrefix(name, "ppt/slides/slide") && strings.HasSuffix(name, ".xml") && !strings.Contains(strings.TrimPrefix(name, "ppt/slides/"), "/") || ext == ".xlsx" && strings.HasPrefix(name, "xl/worksheets/sheet") && strings.HasSuffix(name, ".xml") && !strings.Contains(strings.TrimPrefix(name, "xl/worksheets/"), "/") || strings.HasPrefix(ext, ".od") && name == "content.xml"
		if match {
			entries = append(entries, entry)
		}
	}
	if len(entries) == 0 {
		return "", errors.New("expected document XML was not found (encrypted or unsupported document)")
	}
	sort.Slice(entries, func(i, j int) bool {
		return composerDocumentOrder(entries[i].Name) < composerDocumentOrder(entries[j].Name)
	})
	var totalXML uint64
	for _, entry := range entries {
		if entry.UncompressedSize64 > maxComposerXMLBytes {
			return "", errors.New("document XML entry exceeds 8 MiB")
		}
		totalXML += entry.UncompressedSize64
	}
	if shared != nil {
		if shared.UncompressedSize64 > maxComposerXMLBytes {
			return "", errors.New("document XML entry exceeds 8 MiB")
		}
		totalXML += shared.UncompressedSize64
	}
	if totalXML > 16<<20 {
		return "", errors.New("selected document XML exceeds the 16 MiB total processing limit")
	}
	var out composerTextBuilder
	if ext == ".xlsx" {
		var values []string
		sharedBytes := 0
		if shared != nil {
			var inItem, inText bool
			var item composerTextBuilder
			err = composerXML(ctx, shared, func(token xml.Token) error {
				switch value := token.(type) {
				case xml.StartElement:
					if value.Name.Local == "si" {
						inItem = true
						item = composerTextBuilder{}
					}
					if value.Name.Local == "t" {
						inText = true
					}
				case xml.EndElement:
					if value.Name.Local == "t" {
						inText = false
					}
					if value.Name.Local == "si" {
						inItem = false
						sharedBytes += item.Len()
						if sharedBytes > int(hub.MaxComposerExtractedBytes) {
							return errors.New("shared strings exceed 2 MiB")
						}
						values = append(values, item.String())
						if len(values) > 65536 {
							return errors.New("more than 65536 shared strings are unsupported")
						}
					}
				case xml.CharData:
					if inItem && inText {
						return item.add(string(value))
					}
				}
				return nil
			})
			if err != nil {
				return "", err
			}
			total := 0
			for _, value := range values {
				total += len(value)
				if total > int(hub.MaxComposerExtractedBytes) {
					return "", errors.New("shared strings exceed 2 MiB")
				}
			}
		}
		for _, entry := range entries {
			if err = out.add("[" + entry.Name + "]\n"); err != nil {
				return "", err
			}
			var cellType, cellRef string
			var inValue bool
			var cell composerTextBuilder
			err = composerXML(ctx, entry, func(token xml.Token) error {
				switch value := token.(type) {
				case xml.StartElement:
					switch value.Name.Local {
					case "c":
						cellType = ""
						cellRef = ""
						cell = composerTextBuilder{}
						for _, attr := range value.Attr {
							if attr.Name.Local == "t" {
								cellType = attr.Value
							}
							if attr.Name.Local == "r" {
								cellRef = attr.Value
							}
						}
					case "v", "t":
						inValue = true
					}
				case xml.EndElement:
					switch value.Name.Local {
					case "v", "t":
						inValue = false
					case "c":
						text := cell.String()
						if cellType == "s" {
							index, e := strconv.Atoi(text)
							if e != nil || index < 0 || index >= len(values) {
								return errors.New("invalid spreadsheet shared string reference")
							}
							text = values[index]
						}
						if text != "" {
							return out.add(cellRef + "\t" + text + "\n")
						}
					}
				case xml.CharData:
					if inValue {
						return cell.add(string(value))
					}
				}
				return nil
			})
			if err != nil {
				return "", err
			}
		}
	} else {
		for _, entry := range entries {
			var inText bool
			depth := 0
			err = composerXML(ctx, entry, func(token xml.Token) error {
				switch value := token.(type) {
				case xml.StartElement:
					depth++
					if depth > 256 {
						return errors.New("document XML nesting exceeds 256")
					}
					if ext == ".docx" || ext == ".pptx" {
						if value.Name.Local == "t" {
							inText = true
						}
					} else {
						inText = true
					}
					if value.Name.Local == "tab" {
						return out.add("\t")
					}
				case xml.EndElement:
					depth--
					if value.Name.Local == "t" {
						inText = false
					}
					if value.Name.Local == "p" || value.Name.Local == "tr" || value.Name.Local == "table-row" || value.Name.Local == "h" {
						return out.add("\n")
					}
				case xml.CharData:
					if inText {
						return out.add(string(value))
					}
				}
				return nil
			})
			if err != nil {
				return "", err
			}
			if err = out.add("\n"); err != nil {
				return "", err
			}
		}
	}
	return out.String(), nil
}
func composerDocumentOrder(name string) string {
	base := filepath.Base(name)
	prefix := strings.TrimRight(strings.TrimSuffix(base, ".xml"), "0123456789")
	number, _ := strconv.Atoi(strings.TrimSuffix(strings.TrimPrefix(base, prefix), ".xml"))
	return fmt.Sprintf("%s%08d", prefix, number)
}
