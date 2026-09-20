// Post one click -- down, then up -- at a screen point, with a chord, so a
// live check of a mouse path can be driven without a hand on the trackpad.
// It touches the desktop, so it is guarded: it refuses unless `baud` owns
// the frontmost ordinary window (the check scripts/front.swift makes) and
// the point lies inside that window's bounds. A click that misses the
// window would land on whatever is behind it, and the first version of the
// OSC 8 live check did exactly that.
//
// usage: click-post X Y plain|cmd|cmdshift    (screen points, top-left origin)
//
// Build the way scripts/winid.swift documents.
import CoreGraphics
import Foundation

let a = CommandLine.arguments
guard a.count == 4, let x = Double(a[1]), let y = Double(a[2]) else {
    FileHandle.standardError.write("usage: click-post X Y plain|cmd|cmdshift\n".data(using: .utf8)!)
    exit(2)
}
let flags: CGEventFlags
switch a[3] {
case "plain": flags = []
case "cmd": flags = .maskCommand
case "cmdshift": flags = [.maskCommand, .maskShift]
default:
    FileHandle.standardError.write("unknown chord \(a[3])\n".data(using: .utf8)!)
    exit(2)
}

// The guard: the frontmost ordinary window is baud's and contains the point.
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
guard let w = list.first(where: { ($0["kCGWindowLayer"] as? Int) == 0 }),
      (w["kCGWindowOwnerName"] as? String) == "baud",
      let b = w["kCGWindowBounds"] as? [String: Any],
      let bx = b["X"] as? Double, let by = b["Y"] as? Double,
      let bw = b["Width"] as? Double, let bh = b["Height"] as? Double
else {
    FileHandle.standardError.write("refused: baud does not own the frontmost window\n".data(using: .utf8)!)
    exit(3)
}
guard x >= bx, x < bx + bw, y >= by, y < by + bh else {
    FileHandle.standardError.write("refused: \(x),\(y) is outside baud's window \(bx),\(by),\(bw),\(bh)\n".data(using: .utf8)!)
    exit(3)
}

let p = CGPoint(x: x, y: y)
let src = CGEventSource(stateID: .hidSystemState)
// Move first, so the press lands where AppKit believes the pointer is.
CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?
    .post(tap: .cghidEventTap)
usleep(60_000)
let down = CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left)!
down.flags = flags
down.post(tap: .cghidEventTap)
usleep(60_000)
let up = CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left)!
up.flags = flags
up.post(tap: .cghidEventTap)
print("posted \(a[3]) click at \(x),\(y)")
