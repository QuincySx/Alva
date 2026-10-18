import CoreGraphics
import Foundation

guard (3...4).contains(CommandLine.arguments.count),
      let x = Double(CommandLine.arguments[1]),
      let y = Double(CommandLine.arguments[2]) else {
    fputs("usage: click <x> <y> [left|right]\n", stderr)
    exit(2)
}

let point = CGPoint(x: x, y: y)
let isRightClick = CommandLine.arguments.count == 4 && CommandLine.arguments[3] == "right"
let button: CGMouseButton = isRightClick ? .right : .left
let down: CGEventType = isRightClick ? .rightMouseDown : .leftMouseDown
let up: CGEventType = isRightClick ? .rightMouseUp : .leftMouseUp

CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: button)?.post(tap: .cghidEventTap)
usleep(80_000)
CGEvent(mouseEventSource: nil, mouseType: down, mouseCursorPosition: point, mouseButton: button)?.post(tap: .cghidEventTap)
usleep(60_000)
CGEvent(mouseEventSource: nil, mouseType: up, mouseCursorPosition: point, mouseButton: button)?.post(tap: .cghidEventTap)
