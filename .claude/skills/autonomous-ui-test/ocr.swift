// macOS Vision OCR — reads PNG/JPEG, prints recognized text (one line per
// observation), with optional --json for {bbox, text, confidence} entries.
//
// Usage: swift ocr.swift <image.png> [--json] [--lang en zh]
//
// Defaults: --lang auto (Vision picks); zh-Hans + en explicitly works well.
// Confidence threshold: 0.5 (drops near-noise OCR results).

import Foundation
import Vision
import AppKit

func die(_ msg: String) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(1)
}

// ---- argv -----------------------------------------------------------------
let args = CommandLine.arguments.dropFirst()
guard let path = args.first else { die("usage: ocr.swift <image> [--json] [--lang en zh-Hans]") }
let asJSON = args.contains("--json")
var langs: [String] = []
if let i = args.firstIndex(of: "--lang") {
    langs = Array(args.dropFirst(i + 1)).filter { !$0.hasPrefix("--") }
}
if langs.isEmpty { langs = ["zh-Hans", "en-US"] }

// ---- load image ----------------------------------------------------------
guard let nsimg = NSImage(contentsOfFile: path),
      let cgimg = nsimg.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    die("failed to load image: \(path)")
}

// ---- OCR ------------------------------------------------------------------
let req = VNRecognizeTextRequest()
req.recognitionLanguages = langs
req.recognitionLevel = .accurate
req.usesLanguageCorrection = true

let handler = VNImageRequestHandler(cgImage: cgimg, options: [:])
do { try handler.perform([req]) } catch { die("OCR failed: \(error)") }

guard let observations = req.results else { die("no results") }

// ---- output ---------------------------------------------------------------
struct Entry: Encodable {
    let text: String
    let confidence: Float
    let bbox: [Double]   // [x, y, w, h] in image-normalized coords (origin BL)
}

let entries: [Entry] = observations.compactMap { obs in
    guard let top = obs.topCandidates(1).first, top.confidence >= 0.3 else { return nil }
    let r = obs.boundingBox
    return Entry(
        text: top.string,
        confidence: top.confidence,
        bbox: [Double(r.origin.x), Double(r.origin.y), Double(r.size.width), Double(r.size.height)]
    )
}

if asJSON {
    let enc = JSONEncoder()
    enc.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try enc.encode(entries)
    FileHandle.standardOutput.write(data)
    print()
} else {
    // Sort top→bottom (Vision uses bottom-left origin, so 1.0 is top).
    let sorted = entries.sorted { (a, b) -> Bool in
        let yA = a.bbox[1] + a.bbox[3]
        let yB = b.bbox[1] + b.bbox[3]
        return yA > yB
    }
    for e in sorted {
        print(e.text)
    }
}
