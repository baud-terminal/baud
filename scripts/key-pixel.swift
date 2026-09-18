// The external keystroke-to-pixel probe: post ONE letter to baud and time,
// from outside the app, when the pixels actually change. Its purpose is to
// validate the in-app `vsync` estimate that scripts/key-latency.py reports --
// `vsync` is baud's guess at the pixel time; this is a measurement of it.
//
// HOW IT DETECTS THE CHANGE -- bytes, never idle status
//   The first version waited for ScreenCaptureKit to report a run of idle
//   frames before posting, and it never got one: baud's window repaints
//   continuously (the cause is being looked at separately; assume it stays
//   true), so a probe that keys on the stream's idle/complete status cannot
//   baseline at all. This one does what Typometer does
//   (`[[source: typometer-ChangeDetector]]`: read the pixel, post, poll until
//   the pixel differs) with the window server's own timestamps instead of a
//   poll loop:
//     - one SCStream over baud's window alone (SCContentFilter
//       desktopIndependentWindow), sourceRect a SMALL rect -- the cell to the
//       right of the cursor -- at a 1/120 s minimum frame interval;
//     - baseline = a byte copy of the rect from the FIRST frame, checked
//       against the rect 300 ms later; if they differ, retried up to 5 more
//       times 200 ms apart (a repaint whose phase flipped mid-baseline);
//       never stable -> exit 5;
//     - post the key; the first frame whose rect bytes differ from the
//       baseline is the change, and its SCStreamFrameInfo.displayTime -- "the
//       mach absolute time ... when the frame was displayed by the window
//       server" (`[[source: macos-sdk-scstream-h]]`) -- converted through
//       mach_timebase_info is `pixel`.
//   The stream's idle/complete status is printed for the record and decides
//   nothing.
//
// WHY THAT RECT
//   Before the key the cursor sits at column c; the echo writes the letter at
//   c and moves the cursor to c+1. The rect at c+1 is a blank cell that
//   nothing touches until the echoed frame lands, and then it holds the
//   cursor -- one unambiguous change. The cell at c would hold the cursor
//   before the key, which is a poor baseline. baud's cursor does not blink
//   (the renderer draws BlinkingBlock and SteadyBlock the same; only SGR 5
//   text follows the blink phase), so the rect holds still on both sides of
//   the change; the 500 ms watch after the change counts anything that says
//   otherwise.
//
// COORDINATES -- the driver passes the rect, in WINDOW-RELATIVE POINTS
//   [x y w h] are points relative to the TOP-LEFT corner of baud's window
//   frame -- the rect SCWindow.frame / kCGWindowBounds report, which for
//   baud's FullSizeContentView window includes the titlebar strip -- x to
//   the right, y DOWN. That IS the stream's own space for a window filter:
//   MEASURED on the first live run, SCK takes `sourceRect` for a
//   desktopIndependentWindow filter relative to the window's top-left (the
//   header's "display's logical coordinate system" wording notwithstanding)
//   -- a rect offset by `SCContentFilter.contentRect.origin` (the window's
//   screen frame, 346,123 here) landed ~41 columns and 7 rows off and saw
//   no change for 20 keys, and a rect past the window's width in that
//   space was refused by startCapture (-3812, invalid parameter). So
//       sourceRect = (x, y, w, h), unchanged
//   and contentRect is still printed for the record. The surface is
//   (w, h) x pointPixelScale pixels, BGRA, and the comparison is over every
//   byte of it.
//
//   From baud's own numbers (Responder::cursor_rect speaks the terminal
//   VIEW's top-left points; the view sits `inset` points below the window's
//   top edge -- the "titlebar inset" line baud logs at startup -- and the
//   grid is padded `padding` points in from the view's left and top;
//   cell size and scale are on the BAUD_DEBUG_GEOMETRY line):
//       x = padding + (col + 1) * cell_w / scale
//       y = inset + padding + row * cell_h / scale
//       w = cell_w / scale,  h = cell_h / scale
//   Note that cursor_rect itself omits the padding today; the driver adds
//   it. THE DRIVER SHOULD ALWAYS PASS THE REAL RECT. The default when it is
//   omitted -- 40x40 points at window-relative (10, 30), roughly the start
//   of row 0 below the titlebar -- exists only so the probe runs by hand,
//   and is wrong for any cursor not at the top-left.
//
// TIMEBASE, and the check that catches a unit slip
//   displayTime is in mach ticks (on Apple silicon numer/denom = 125/3, so
//   ticks are NOT nanoseconds); `post` is clock_gettime_nsec_np
//   (CLOCK_UPTIME_RAW), which the man page defines as mach_absolute_time
//   after the same timebase conversion -- one clock, once converted. The
//   probe prints the timebase, the raw displayTime, the arrival
//   mach_absolute_time and the arrival CLOCK_UPTIME_RAW of the change frame
//   beside the converted value, so which unit arrived is visible; and it
//   REFUSES (exit 5) a delta <= 0 or > 1 s: a conversion applied the wrong
//   way round reads as tens of seconds and would otherwise pass.
//
// POSITIVE CONTROL, built in
//   After the change is seen no further key is posted and the rect is
//   watched for 500 ms more; every frame whose rect differs from the frame
//   before it is counted, and a non-zero count prints
//   `WARNING rect changed again without a key: <n> times` -- the change
//   could have been that. It does not change the exit code.
//
// SAFETY
//   The same guard as key-post.swift, run THREE times: before anything
//   (before the stream exists), before the key-down, and before the key-up.
//   The frontmost layer-0 window's owner AND NSWorkspace's frontmost
//   application must both be <pid>, or it prints `REFUSED front window
//   owner <pid'> <name> front app <pid''> <name> posted <k>` and exits 3
//   having posted nothing further (<k> is 0, or 1 when the key-down had
//   gone and the key-up was withheld). One plain ASCII letter, `flags`
//   set to [] on both events, never Return, a modifier or a control key; a
//   modifier PHYSICALLY held during a run is outside the guard. Screen
//   Recording is preflighted, never requested (no desktop dialog). The
//   probe does not activate baud; the driver raises it, as it does for
//   key-post.swift.
//
// Run it the way scripts/winid.swift documents (this box's PATH `swift` is a
// nix xcode-select shim with no compiler):
//   DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//     swift -sdk <MacOSX.sdk> scripts/key-pixel.swift <pid> <char> [x y w h]
//
// OUTPUT (stdout; `post`, `pixel` and `delta_us` are exactly two tokens)
//   timebase <numer>/<denom>
//   window id=<id> frame=<x>,<y> <w>x<h> title=<...> of <n> candidates
//   rect window-relative <x> <y> <w> <h> -> sourceRect <x> <y> <w> <h> contentRect <...> pointPixelScale <s>
//   frame0 <W>x<H> px status=<s> displayTime_raw=<ticks> arrival_mach=<ticks> contentRect=<...>
//   baseline stable after <k> attempt(s), <n> frames seen, pixel0 BGRA=<b>,<g>,<r>,<a>
//   post <ns>
//   pixel <ns>
//   pixel_raw displayTime=<ticks> arrival_mach=<ticks> arrival_ns=<ns> pts_ns=<ns> status=<s>
//   delta_us <n>
//   WARNING rect changed again without a key: <n> times      (only if n > 0)
//
// EXIT CODES
//   0  measured a pixel change after the key
//   2  bad args / non-letter / rect outside the window / Screen Recording not granted
//   3  REFUSED: baud did not own the front window or was not the front
//      application; nothing further was posted (the line says how many were)
//   4  ScreenCaptureKit unavailable (no window for the pid, no frames, start failed)
//   5  positive control failed: rect never stable, no change seen, or a delta
//      that cannot be a keystroke (<= 0 or > 1 s: unit mismatch or self-change)
import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

setbuf(stdout, nil)

func die(_ msg: String, _ code: Int32) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

// ---- arguments ---------------------------------------------------------
let a = CommandLine.arguments
guard a.count == 3 || a.count == 7, let pid = Int32(a[1]) else {
    die("usage: key-pixel.swift <pid> <char> [x y w h]   (x y w h: window-relative points, y down)", 2)
}
let chChars = Array(a[2])
guard chChars.count == 1, let ch = chChars.first, ch.isASCII, ch.isLetter else {
    die("char must be a single ASCII letter (never Return/control)", 2)
}

// The wanted rect, window-relative points, y down. The default is a stand-in
// for running by hand; the driver passes the real cell (see the header).
struct WinRect { var x: Double; var y: Double; var w: Double; var h: Double }
var want = WinRect(x: 10, y: 30, w: 40, h: 40)
var rectGiven = false
if a.count == 7 {
    guard let x = Double(a[3]), let y = Double(a[4]),
          let w = Double(a[5]), let h = Double(a[6]), w > 0, h > 0 else {
        die("x y w h must be numbers with w, h > 0", 2)
    }
    want = WinRect(x: x, y: y, w: w, h: h)
    rectGiven = true
}

// ---- clocks: one, mach host time -----------------------------------------
func hostNow() -> UInt64 { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
var tb = mach_timebase_info()
mach_timebase_info(&tb)
let tbNumer = UInt64(tb.numer), tbDenom = UInt64(tb.denom)
/// mach ticks -> nanoseconds, exact integer arithmetic (no UInt128 needed).
func machToNs(_ t: UInt64) -> UInt64 {
    (t / tbDenom) * tbNumer + ((t % tbDenom) * tbNumer) / tbDenom
}
print("timebase \(tb.numer)/\(tb.denom)")

// ---- the guard, identical to key-post.swift ------------------------------
// The frontmost ordinary (layer-0) window's owner pid and name, or nil.
func frontOwner() -> (pid: Int32, name: String)? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID)
        as? [[String: Any]] ?? []
    for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
        return (w["kCGWindowOwnerPID"] as? Int32 ?? -1,
                w["kCGWindowOwnerName"] as? String ?? "?")
    }
    return nil
}

// The frontmost application's pid and name (-1, "?" when none).
func frontApp() -> (pid: Int32, name: String) {
    let app = NSWorkspace.shared.frontmostApplication
    return (app?.processIdentifier ?? -1, app?.localizedName ?? "?")
}

// How many key-downs have been posted, so a refusal can say what preceded it.
var posted = 0

/// The guard, key-post.swift's: the frontmost layer-0 window's owner AND the
/// frontmost application must both be <pid> (posted HID events go to the
/// active application's key window, which can differ from the front window's
/// owner). Refusing exits 3 having posted nothing further; `posted` says
/// whether the key-down had already gone (0 or 1 here).
func guardFront() {
    let f = frontOwner()
    let app = frontApp()
    if let f = f, f.pid == pid, app.pid == pid { return }
    print("REFUSED front window owner \(f?.pid ?? -1) \(f?.name ?? "?") "
          + "front app \(app.pid) \(app.name) posted \(posted)")
    exit(3)
}

// 1. Guard before anything: no stream, no capture, no post unless baud is
//    frontmost. A bogus pid stops here.
guardFront()

// 2. Screen Recording: preflight only. Request would raise a desktop dialog.
guard CGPreflightScreenCaptureAccess() else {
    die("Screen Recording not granted (CGPreflightScreenCaptureAccess=false)", 2)
}

// ---- 3. baud's window: the largest on-screen window the pid owns ---------
let contentSem = DispatchSemaphore(value: 0)
var content: SCShareableContent?
var contentErr: String?
SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) { c, e in
    if let e = e { contentErr = "\(e)" } else { content = c }
    contentSem.signal()
}
if contentSem.wait(timeout: .now() + 5) == .timedOut { die("SCShareableContent timed out", 4) }
if let e = contentErr { die("SCShareableContent: \(e)", 4) }
guard let content = content else { die("SCShareableContent: nil", 4) }

func area(_ r: CGRect) -> CGFloat { r.width * r.height }
let candidates = content.windows.filter { $0.owningApplication?.processID == pid }
guard let win = candidates.max(by: { area($0.frame) < area($1.frame) }) else {
    die("no on-screen window owned by pid \(pid) (is baud on this Space?)", 4)
}
print("window id=\(win.windowID) frame=\(win.frame.minX),\(win.frame.minY) "
      + "\(win.frame.width)x\(win.frame.height) title=\(win.title ?? "") of \(candidates.count) candidates")

// ---- 4. the filter, the scale, the rect in the stream's space ------------
let filter = SCContentFilter(desktopIndependentWindow: win)
let content_rect = filter.contentRect
var scale = Int(filter.pointPixelScale.rounded())
if scale < 1 {
    // Older SCK: derive the scale from the display the window sits on.
    let disp = content.displays.first(where: { $0.frame.contains(win.frame.origin) })
        ?? content.displays.first
    if let d = disp, d.frame.width > 0 {
        scale = max(1, Int((Double(d.width) / Double(d.frame.width)).rounded()))
    } else {
        scale = 2
    }
}
guard want.x >= 0, want.y >= 0,
      want.x + want.w <= Double(win.frame.width) + 0.5,
      want.y + want.h <= Double(win.frame.height) + 0.5 else {
    die("rect \(want.x) \(want.y) \(want.w) \(want.h) lies outside the window "
        + "(\(win.frame.width)x\(win.frame.height) points)", 2)
}
// The window's OWN space: the first live run (2026-09-18, lane V2) showed
// that a rect offset by contentRect.origin -- the window's screen frame --
// landed ~41 columns and 7 rows away from the cursor (20 keys, no change
// seen), and a rect past the window's width in that space was refused by
// startCapture with SCStreamErrorDomain -3812 "invalid parameter". So
// sourceRect is window-relative, y down, exactly as [x y w h] arrive.
let source = CGRect(x: want.x, y: want.y, width: want.w, height: want.h)
print("rect window-relative \(want.x) \(want.y) \(want.w) \(want.h)"
      + (rectGiven ? "" : " (DEFAULT -- the driver should pass the real cell)")
      + " -> sourceRect \(source.minX) \(source.minY) \(source.width) \(source.height)"
      + " contentRect \(content_rect.minX),\(content_rect.minY) \(content_rect.width)x\(content_rect.height)"
      + " pointPixelScale \(scale)")

// ---- 5. the stream ------------------------------------------------------
let cfg = SCStreamConfiguration()
cfg.sourceRect = source
cfg.width = Int((want.w * Double(scale)).rounded())
cfg.height = Int((want.h * Double(scale)).rounded())
cfg.pixelFormat = kCVPixelFormatType_32BGRA
cfg.minimumFrameInterval = CMTime(value: 1, timescale: 120)
cfg.queueDepth = 8
cfg.showsCursor = false

/// A byte copy of the whole surface (which IS the rect: sourceRect and the
/// output size were set to it), row by row so the row padding is left out.
func copyRegion(_ pb: CVPixelBuffer) -> [UInt8] {
    CVPixelBufferLockBaseAddress(pb, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(pb, .readOnly) }
    guard let base = CVPixelBufferGetBaseAddress(pb) else { return [] }
    let w = CVPixelBufferGetWidth(pb), h = CVPixelBufferGetHeight(pb)
    let bpr = CVPixelBufferGetBytesPerRow(pb)
    let rowBytes = w * 4
    var out = [UInt8](repeating: 0, count: rowBytes * h)
    out.withUnsafeMutableBytes { dst in
        guard let d = dst.baseAddress else { return }
        for y in 0..<h {
            memcpy(d + y * rowBytes, base + y * bpr, rowBytes)
        }
    }
    return out
}

struct Change {
    let bytes: [UInt8]
    let displayTicks: UInt64
    let arrivalTicks: UInt64
    let arrivalNs: UInt64
    let ptsNs: Int64
    let status: Int
}

final class Cap: NSObject, SCStreamOutput, SCStreamDelegate {
    let lock = NSLock()
    var frames = 0
    var first: [UInt8]?          // the FIRST frame's rect bytes
    var firstLine: String?       // what the first frame said about itself
    var latest: [UInt8]?         // the most recent frame's rect bytes
    var baseline: [UInt8]?       // set once the rect proved stable
    var armed = false            // true from just before the post
    var change: Change?          // the first post-arm frame that differs
    var again = 0                // frames after the change that differ from the one before
    var stopErr: String?
    let firstFrame = DispatchSemaphore(value: 0)
    let changed = DispatchSemaphore(value: 0)

    func snapshotLatest() -> [UInt8]? { lock.lock(); defer { lock.unlock() }; return latest }
    func snapshotFirst() -> [UInt8]? { lock.lock(); defer { lock.unlock() }; return first }
    func frameCount() -> Int { lock.lock(); defer { lock.unlock() }; return frames }
    func setBaseline(_ b: [UInt8]) { lock.lock(); baseline = b; lock.unlock() }
    func arm() { lock.lock(); armed = true; lock.unlock() }
    func takeChange() -> Change? { lock.lock(); defer { lock.unlock() }; return change }
    func againCount() -> Int { lock.lock(); defer { lock.unlock() }; return again }

    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer,
                of type: SCStreamOutputType) {
        guard type == .screen else { return }
        let arrivalTicks = mach_absolute_time()
        let arrivalNs = hostNow()
        // A frame without pixels (an idle frame on some builds) carries
        // nothing to compare; it is neither a change nor a baseline.
        guard let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        let bytes = copyRegion(pb)
        if bytes.isEmpty { return }
        var status = -1
        var dt: UInt64 = 0
        var crect = "-"
        if let arr = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]], let info = arr.first {
            status = (info[.status] as? NSNumber)?.intValue ?? -1
            dt = (info[.displayTime] as? NSNumber)?.uint64Value ?? 0
            if let d = info[.contentRect] as? NSDictionary,
               let r = CGRect(dictionaryRepresentation: d as CFDictionary) {
                crect = "\(r.minX),\(r.minY) \(r.width)x\(r.height)"
            }
        }
        let pts = CMSampleBufferGetPresentationTimeStamp(sb)
        let ptsNs = pts.isValid ? Int64((CMTimeGetSeconds(pts) * 1e9).rounded()) : -1

        lock.lock(); defer { lock.unlock() }
        frames += 1
        let prev = latest
        latest = bytes
        if frames == 1 {
            first = bytes
            firstLine = "frame0 \(CVPixelBufferGetWidth(pb))x\(CVPixelBufferGetHeight(pb)) px "
                + "status=\(status) displayTime_raw=\(dt) arrival_mach=\(arrivalTicks) contentRect=\(crect)"
            firstFrame.signal()
        }
        if change == nil {
            if armed, let base = baseline, bytes != base {
                change = Change(bytes: bytes, displayTicks: dt, arrivalTicks: arrivalTicks,
                                arrivalNs: arrivalNs, ptsNs: ptsNs, status: status)
                changed.signal()
            }
        } else if let p = prev, p != bytes {
            again += 1
        }
    }
    func stream(_ s: SCStream, didStopWithError e: Error) {
        lock.lock(); stopErr = "\(e)"; lock.unlock()
    }
}

let cap = Cap()
let stream = SCStream(filter: filter, configuration: cfg, delegate: cap)
do {
    try stream.addStreamOutput(cap, type: .screen, sampleHandlerQueue: DispatchQueue(label: "cap"))
} catch { die("addStreamOutput: \(error)", 4) }

func stopStream() {
    let s = DispatchSemaphore(value: 0)
    stream.stopCapture { _ in s.signal() }
    _ = s.wait(timeout: .now() + 2)
}

let startSem = DispatchSemaphore(value: 0)
var startErr: String?
stream.startCapture { e in if let e = e { startErr = "\(e)" }; startSem.signal() }
_ = startSem.wait(timeout: .now() + 5)
if let e = startErr { die("startCapture: \(e)", 4) }

// ---- 6. baseline: the first frame's bytes, held still for 300 ms --------
if cap.firstFrame.wait(timeout: .now() + 3) == .timedOut {
    stopStream()
    die("no frame from the stream within 3 s" + (cap.stopErr.map { " (stream stopped: \($0))" } ?? ""), 4)
}
print(cap.firstLine ?? "frame0 ?")

let maxAttempts = 1 + 5   // the first try, then up to 5 retries
var attempts = 0
var stable: [UInt8]? = nil
while attempts < maxAttempts {
    attempts += 1
    // Attempt 1 honours "the FIRST frame's pixels"; a retry starts from the
    // latest frame, because the first one has been overtaken by then.
    let sampleA = attempts == 1 ? cap.snapshotFirst() : cap.snapshotLatest()
    Thread.sleep(forTimeInterval: 0.3)
    let sampleB = cap.snapshotLatest()
    if let sa = sampleA, let sb = sampleB, sa == sb {
        stable = sb
        break
    }
    if attempts < maxAttempts { Thread.sleep(forTimeInterval: 0.2) }
}
guard let base = stable else {
    stopStream()
    die("rect never stable after \(attempts) attempts (\(cap.frameCount()) frames seen): "
        + "it changes on its own; cannot attribute a change to the key", 5)
}
cap.setBaseline(base)
let p0 = base.count >= 4 ? "\(base[0]),\(base[1]),\(base[2]),\(base[3])" : "?"
print("baseline stable after \(attempts) attempt(s), \(cap.frameCount()) frames seen, pixel0 BGRA=\(p0)")

// ---- 7. the key: guard, stamp, post; guard, post the up ------------------
guardFront()  // refuse before building or posting anything
var u: [UniChar] = Array(String(ch).utf16)
let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true)!
down.keyboardSetUnicodeString(stringLength: u.count, unicodeString: &u)
down.flags = []  // a plain letter: no modifier of the event's own
let postNs = hostNow()
down.timestamp = CGEventTimestamp(postNs)
cap.arm()
print("post \(postNs)")
down.post(tap: .cghidEventTap)
posted += 1
Thread.sleep(forTimeInterval: 0.001)  // 1 ms, then the key-up
guardFront()  // again: the front can change in that millisecond
let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)!
up.keyboardSetUnicodeString(stringLength: u.count, unicodeString: &u)
up.flags = []
up.post(tap: .cghidEventTap)

// ---- 8. the change ------------------------------------------------------
if cap.changed.wait(timeout: .now() + 2) == .timedOut {
    stopStream()
    die("no change seen within 2 s of the post (\(cap.frameCount()) frames): wrong rect, or the "
        + "key did not land" + (cap.stopErr.map { " (stream stopped: \($0))" } ?? ""), 5)
}
guard let c = cap.takeChange() else { stopStream(); die("no change recorded", 5) }
let pixelNs = machToNs(c.displayTicks)
print("pixel \(pixelNs)")
// The raw stamps beside the converted one, so which unit arrived is visible:
// displayTime and arrival_mach are both mach ticks and should be close;
// arrival_ns is CLOCK_UPTIME_RAW on the same base as `post` and `pixel`.
print("pixel_raw displayTime=\(c.displayTicks) arrival_mach=\(c.arrivalTicks) "
      + "arrival_ns=\(c.arrivalNs) pts_ns=\(c.ptsNs) status=\(c.status)")
let delta = Int64(bitPattern: pixelNs &- postNs)
print("delta_us \(Double(delta) / 1000.0)")

// ---- 9. positive control: 500 ms with no key -----------------------------
Thread.sleep(forTimeInterval: 0.5)
let again = cap.againCount()
stopStream()
if again > 0 {
    print("WARNING rect changed again without a key: \(again) times "
          + "(a repaint or blink in that cell; the change above could have been that)")
}

// ---- 10. refuse a delta that cannot be a keystroke -----------------------
if delta <= 0 {
    die("pixel time not after post (delta \(delta) ns): the rect changed on its own at the "
        + "post, or displayTime and CLOCK_UPTIME_RAW do not agree -- unit mismatch", 5)
}
if delta > 1_000_000_000 {
    die("delta \(delta) ns exceeds 1 s: unit mismatch (a timebase applied the wrong way "
        + "reads as tens of seconds), not a keystroke", 5)
}
exit(0)
