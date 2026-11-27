// Post the pointer gestures the README's tools clip is made of -- a wheel
// turned a line at a time, a drag, a click -- into one baud window, so the
// clip is driven by a script and not by a hand on the trackpad
// (tools/scripts/readme-capture.sh drives it).
//
// It touches the desktop, so it is guarded harder than
// tools/scripts/click-post.swift: before every event it requires that the
// frontmost ordinary window belongs to the **pid** it was given, not just to
// a process named baud, and that the point lies inside that window. The
// user's own baud is also named baud, and a gesture that passed a name
// check would land in it.
//
// usage: readme-pointer <pid> wheel X Y LINES     positive shows earlier lines
//        readme-pointer <pid> drag X0 Y0 X1 Y1
//        readme-pointer <pid> click X Y
// (screen points, top-left origin). Exits 3 when the guard refuses.
//
// Build the way tools/scripts/winid.swift documents.
import CoreGraphics
import Foundation

func die(_ m: String, _ code: Int32) -> Never {
    FileHandle.standardError.write(("readme-pointer: " + m + "\n").data(using: .utf8)!)
    exit(code)
}

let a = CommandLine.arguments
guard a.count >= 3, let pid = Int32(a[1]) else {
    die("usage: readme-pointer <pid> wheel X Y LINES | drag X0 Y0 X1 Y1 | click X Y", 2)
}
let nums = a.dropFirst(3).map { Double($0) }
guard !nums.contains(where: { $0 == nil }) else { die("not a number in \(a.dropFirst(3))", 2) }
let n = nums.map { $0! }

/// Refuses unless `p` lies in a window of `pid` with no other process's
/// ordinary window in front of it. The windows are walked front to back
/// while `pid` owns them: the process's own tooltip can be the frontmost,
/// as it was over the title when this was first run, and is passed over.
func guardPoint(_ p: CGPoint) {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
        guard (w["kCGWindowOwnerPID"] as? Int32) == pid else {
            die("refused: pid \(pid) does not own the frontmost window (\(w["kCGWindowOwnerName"] ?? "?") does)", 3)
        }
        if let b = w["kCGWindowBounds"] as? [String: Any],
           let bx = b["X"] as? Double, let by = b["Y"] as? Double,
           let bw = b["Width"] as? Double, let bh = b["Height"] as? Double,
           p.x >= bx, p.x < bx + bw, p.y >= by, p.y < by + bh {
            return
        }
    }
    die("refused: \(p.x),\(p.y) is in no window of pid \(pid)", 3)
}

let src = CGEventSource(stateID: .hidSystemState)
func post(_ e: CGEvent?, at p: CGPoint) {
    guardPoint(p)
    e?.post(tap: .cghidEventTap)
}
func mouse(_ t: CGEventType, _ p: CGPoint) -> CGEvent? {
    CGEvent(mouseEventSource: src, mouseType: t, mouseCursorPosition: p, mouseButton: .left)
}

switch a[2] {
case "wheel" where n.count == 3:
    let p = CGPoint(x: n[0], y: n[1])
    post(mouse(.mouseMoved, p), at: p)
    usleep(60_000)
    // One line an event, 40 ms apart: a wheel turned steadily, which the
    // screen shows line by line rather than as one jump.
    let lines = Int32(n[2])
    for _ in 0..<abs(lines) {
        let e = CGEvent(scrollWheelEvent2Source: src, units: .line, wheelCount: 1,
                        wheel1: lines > 0 ? 1 : -1, wheel2: 0, wheel3: 0)
        e?.location = p
        post(e, at: p)
        usleep(40_000)
    }
case "drag" where n.count == 4:
    let p0 = CGPoint(x: n[0], y: n[1]), p1 = CGPoint(x: n[2], y: n[3])
    post(mouse(.mouseMoved, p0), at: p0)
    usleep(60_000)
    post(mouse(.leftMouseDown, p0), at: p0)
    // Twenty-five steps over about 0.6 s, so the selection grows on film.
    for i in 1...25 {
        let t = Double(i) / 25
        let p = CGPoint(x: p0.x + (p1.x - p0.x) * t, y: p0.y + (p1.y - p0.y) * t)
        post(mouse(.leftMouseDragged, p), at: p)
        usleep(24_000)
    }
    post(mouse(.leftMouseUp, p1), at: p1)
case "click" where n.count == 2:
    let p = CGPoint(x: n[0], y: n[1])
    post(mouse(.mouseMoved, p), at: p)
    usleep(60_000)
    post(mouse(.leftMouseDown, p), at: p)
    usleep(60_000)
    post(mouse(.leftMouseUp, p), at: p)
default:
    die("usage: readme-pointer <pid> wheel X Y LINES | drag X0 Y0 X1 Y1 | click X Y", 2)
}
print("posted \(a[2]) into pid \(pid)")
