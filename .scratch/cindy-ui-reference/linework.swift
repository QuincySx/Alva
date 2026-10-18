import AppKit
import CoreImage
import Foundation

guard CommandLine.arguments.count == 3 else {
    fputs("usage: linework <input-directory> <output-directory>\n", stderr)
    exit(2)
}

let fileManager = FileManager.default
let inputDirectory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let outputDirectory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
try fileManager.createDirectory(at: outputDirectory, withIntermediateDirectories: true)

let context = CIContext(options: [.useSoftwareRenderer: false])
let colorSpace = CGColorSpaceCreateDeviceRGB()
let inputs = try fileManager.contentsOfDirectory(
    at: inputDirectory,
    includingPropertiesForKeys: nil,
    options: [.skipsHiddenFiles]
).filter { $0.pathExtension.lowercased() == "png" }.sorted { $0.lastPathComponent < $1.lastPathComponent }

for input in inputs {
    guard let source = CIImage(contentsOf: input) else {
        fputs("cannot load \(input.path)\n", stderr)
        exit(1)
    }

    let edges = source.applyingFilter(
        "CIEdges",
        parameters: [kCIInputIntensityKey: 3.2]
    )
    let inverted = edges.applyingFilter("CIColorInvert")
    let linework = inverted.applyingFilter(
        "CIColorControls",
        parameters: [
            kCIInputSaturationKey: 0.0,
            kCIInputBrightnessKey: -0.02,
            kCIInputContrastKey: 1.9,
        ]
    ).cropped(to: source.extent)

    guard let image = context.createCGImage(
        linework,
        from: source.extent,
        format: .RGBA8,
        colorSpace: colorSpace
    ) else {
        fputs("cannot render \(input.path) extent=\(source.extent) output=\(linework.extent)\n", stderr)
        exit(1)
    }

    let bitmap = NSBitmapImageRep(cgImage: image)
    guard let data = bitmap.representation(using: .png, properties: [:]) else {
        fputs("cannot encode \(input.path)\n", stderr)
        exit(1)
    }

    let output = outputDirectory.appendingPathComponent(input.lastPathComponent)
    try data.write(to: output, options: .atomic)
}

print("rendered \(inputs.count) linework frames")
