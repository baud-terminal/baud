// What AppKit's own overlay scroller does, measured on a scroll view of this
// probe's own: when the knob appears after each trigger, how long it holds,
// how long it fades and in what shape, how wide it is at rest, what colour
// and alpha it draws in each appearance, and what a legacy scroller draws
// and where it is laid out. `docs/research/notes/l-scrollbar.md` quotes the numbers; nothing
// here is recalled, because AppKit's header documents none of them
// (NSScroller.h and NSScrollView.h name `flashScrollers` "momentarily").
//
// NOTHING IS POSTED TO THE DESKTOP. Every trigger is an in-process call on
// the probe's own views -- `flashScrollers()`, `scrollWheel(with:)` on an
// NSEvent built from a CGEvent that is never posted, and a programmatic
// scroll. The cursor does not move and no other application sees an event. The window is
// ordered in without activating the probe, at the screen's lower left.
//
// HOW A NUMBER IS TAKEN
//   One SCStream over this window alone, sourceRect the rightmost strip of
//   the content, 1/120 s minimum frame interval, each frame's
//   SCStreamFrameInfo.displayTime -- the window server's own time -- in mach
//   ticks. The baseline is a frame taken while nothing is shown; a frame's
//   "ink" is its summed absolute difference from the baseline. Per trial:
//     appear   first frame after the trigger whose ink passes 2% of the peak
//     hold     last frame at 97% of the peak or above
//     gone     first frame after the hold back under 2% of the peak
//   all as milliseconds after the trigger was called. Six trials a case,
//   three seconds apart, the first discarded; the median and range of the
//   rest are printed. A case whose knob never appears says so: that is its
//   positive control, not a zero.
//
//   Colour and alpha are solved from the peak frame over a black and over a
//   white document: alpha = 1 - (W - B)/255, colour = B / alpha, per channel,
//   at the knob's central pixel -- located on whichever background shows it,
//   because a light knob over a white document has no contrast.
//
// WHAT IT CANNOT DRIVE, and says so rather than reporting a zero
//   - A phased trackpad gesture built in-process reveals the knob once and
//     never again: its positive control fails, so it is no measurement.
//   - Hover: neither the scroll view nor its scroller exposes a tracking area
//     to send `mouseEntered` to, and a real move would take the user's
//     cursor. The legacy scroller -- what an overlay one widens into -- is
//     read in its place, and the note labels it as that proxy.
//   - Each case runs on a fresh scroll view: a view that has been flashed,
//     wheeled and scrolled carries state, and an earlier version saw every
//     flash after its second setup read nothing.
//
// Build and run (no arguments):
//   X=/Applications/Xcode.app/Contents/Developer
//   DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o scroller-probe scripts/scroller-probe.swift && ./scroller-probe

import AppKit
import CoreMedia
import ScreenCaptureKit

func die(_ m: String, _ code: Int32) -> Never {
    FileHandle.standardError.write((m + "\n").data(using: .utf8)!)
    exit(code)
}

var tb = mach_timebase_info()
mach_timebase_info(&tb)
func ms(_ ticks: UInt64) -> Double { Double(ticks) * Double(tb.numer) / Double(tb.denom) / 1e6 }

// ---- the probe's own window --------------------------------------------

final class Solid: NSView {
    var color: NSColor = .black { didSet { needsDisplay = true } }
    override var isFlipped: Bool { true }
    override func draw(_ r: NSRect) {
        color.setFill()
        r.fill()
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let contentSize = NSSize(width: 240, height: 520)
let window = NSWindow(
    contentRect: NSRect(origin: NSPoint(x: 40, y: 40), size: contentSize),
    styleMask: [.titled], backing: .buffered, defer: false)
window.title = "scroller-probe"
// A fresh scroll view for every case: one that has been flashed, wheeled
// and scrolled carries state, and the first version of this probe saw every
// flash after its second setup read nothing.
var scroll = NSScrollView(frame: NSRect(origin: .zero, size: contentSize))
var doc = Solid(frame: NSRect(x: 0, y: 0, width: contentSize.width, height: 20_000))
func fresh(_ color: NSColor, _ st: NSScroller.Style) {
    scroll = NSScrollView(frame: NSRect(origin: .zero, size: contentSize))
    scroll.hasVerticalScroller = true
    scroll.hasHorizontalScroller = false
    scroll.autohidesScrollers = false
    scroll.drawsBackground = true
    scroll.scrollerStyle = st
    scroll.backgroundColor = color
    doc = Solid(frame: NSRect(x: 0, y: 0, width: contentSize.width, height: 20_000))
    doc.color = color
    scroll.documentView = doc
    window.contentView = scroll
    scroll.contentView.scroll(to: NSPoint(x: 0, y: 10_000))
    scroll.reflectScrolledClipView(scroll.contentView)
}
fresh(.black, .overlay)

// ---- the stream ---------------------------------------------------------

struct Frame {
    let ticks: UInt64
    let bytes: [UInt8]
}

final class Cap: NSObject, SCStreamOutput, SCStreamDelegate {
    let lock = NSLock()
    var frames: [Frame] = []
    var recording = false
    var w = 0, h = 0
    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        var dt: UInt64 = 0
        if let arr = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false)
            as? [[SCStreamFrameInfo: Any]], let info = arr.first {
            dt = (info[.displayTime] as? NSNumber)?.uint64Value ?? 0
        }
        CVPixelBufferLockBaseAddress(pb, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pb, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pb) else { return }
        let fw = CVPixelBufferGetWidth(pb), fh = CVPixelBufferGetHeight(pb)
        let bpr = CVPixelBufferGetBytesPerRow(pb)
        var out = [UInt8](repeating: 0, count: fw * fh * 4)
        out.withUnsafeMutableBytes { d in
            for y in 0..<fh { memcpy(d.baseAddress! + y * fw * 4, base + y * bpr, fw * 4) }
        }
        lock.lock()
        w = fw; h = fh
        if recording { frames.append(Frame(ticks: dt == 0 ? mach_absolute_time() : dt, bytes: out)) }
        latest = out
        lock.unlock()
    }
    var latest: [UInt8] = []
    func start() { lock.lock(); frames = []; recording = true; lock.unlock() }
    func stop() -> [Frame] { lock.lock(); recording = false; let f = frames; lock.unlock(); return f }
    func now() -> [UInt8] { lock.lock(); defer { lock.unlock() }; return latest }
    func size() -> (Int, Int) { lock.lock(); defer { lock.unlock() }; return (w, h) }
}

let cap = Cap()
var stream: SCStream?
var scale = 2

func main_(_ f: () -> Void) { DispatchQueue.main.sync(execute: f) }
func sleep_(_ s: Double) { Thread.sleep(forTimeInterval: s) }

func ink(_ a: [UInt8], _ b: [UInt8]) -> Int {
    var s = 0
    let n = min(a.count, b.count)
    var i = 0
    while i < n { s += abs(Int(a[i]) - Int(b[i])); i += 1 }
    return s
}

struct Trial {
    let appear: Double?
    let hold: Double?
    let fading: Double?
    let gone: Double?
    let peak: Int
    let peakFrame: [UInt8]
}

/// One trigger, recorded from just before it for `span` seconds.
func trial(_ trigger: @escaping () -> Void, span: Double = 2.8) -> Trial {
    let baseline = cap.now()
    cap.start()
    sleep_(0.05)
    var t0: UInt64 = 0
    main_ { trigger(); t0 = mach_absolute_time() }
    sleep_(span)
    let fs = cap.stop().filter { $0.ticks >= t0 }
    let inks = fs.map { ink($0.bytes, baseline) }
    guard let peak = inks.max(), peak > 0 else {
        return Trial(appear: nil, hold: nil, fading: nil, gone: nil, peak: 0, peakFrame: baseline)
    }
    let pi = inks.firstIndex(of: peak)!
    let lo = peak / 50
    let appear = inks.firstIndex(where: { $0 > lo }).map { ms(fs[$0].ticks - t0) }
    var holdIdx = pi
    for i in pi..<inks.count where inks[i] * 100 >= peak * 97 { holdIdx = i }
    let goneIdx = (holdIdx..<inks.count).first(where: { inks[$0] <= lo })
    // A frame arrives only when the pixels change, so the hold ends between
    // the last frame at the peak and the first frame below it.
    let fadingIdx = (holdIdx..<inks.count).first(where: { inks[$0] * 100 < peak * 97 })
    return Trial(appear: appear, hold: ms(fs[holdIdx].ticks - t0),
                 fading: fadingIdx.map { ms(fs[$0].ticks - t0) },
                 gone: goneIdx.map { ms(fs[$0].ticks - t0) }, peak: peak, peakFrame: fs[pi].bytes)
}

func stat(_ xs: [Double?]) -> String {
    let v = xs.compactMap { $0 }.sorted()
    if v.isEmpty { return "never" }
    let med = v[v.count / 2]
    return String(format: "%.0f [%.0f-%.0f] (%d of %d)", med, v.first!, v.last!, v.count, xs.count)
}

/// Six trials of one trigger, the first discarded.
func trials(_ name: String, _ trigger: @escaping () -> Void, span: Double = 2.8) -> [Trial] {
    var ts: [Trial] = []
    for _ in 0..<6 {
        ts.append(trial(trigger, span: span))
        sleep_(3.0 - min(span, 2.9) + 0.4)
    }
    let kept = Array(ts.dropFirst())
    print("\(name):  (peak ink per trial, the first discarded: "
          + ts.map { String($0.peak) }.joined(separator: " ") + ")")
    print("   appear ms \(stat(kept.map { $0.appear }))")
    print("   hold   ms \(stat(kept.map { $0.hold }))  (last frame at 97% of the peak)")
    print("   fading ms \(stat(kept.map { $0.fading }))  (first frame below it)")
    print("   gone   ms \(stat(kept.map { $0.gone }))")
    return kept
}

/// The bounding box of pixels that differ from `base` by more than 24 in a
/// channel, in pixels of the strip: (minX, minY, maxX, maxY).
func box(_ f: [UInt8], _ base: [UInt8]) -> (Int, Int, Int, Int)? {
    let (w, h) = cap.size()
    var b: (Int, Int, Int, Int)?
    for y in 0..<h {
        for x in 0..<w {
            let i = (y * w + x) * 4
            let d = (0..<3).map { abs(Int(f[i + $0]) - Int(base[i + $0])) }.max()!
            if d > 24 {
                if let c = b { b = (min(c.0, x), min(c.1, y), max(c.2, x), max(c.3, y)) } else { b = (x, y, x, y) }
            }
        }
    }
    return b
}

/// The median BGR of the central third of `bx` in frame `f`.
func centre(_ f: [UInt8], _ bx: (Int, Int, Int, Int)) -> [Double] {
    let (w, _) = cap.size()
    let cx0 = bx.0 + (bx.2 - bx.0) / 3, cx1 = bx.2 - (bx.2 - bx.0) / 3
    let cy0 = bx.1 + (bx.3 - bx.1) / 3, cy1 = bx.3 - (bx.3 - bx.1) / 3
    var ch: [[Int]] = [[], [], []]
    for y in cy0...max(cy0, cy1) {
        for x in cx0...max(cx0, cx1) {
            let i = (y * w + x) * 4
            for c in 0..<3 { ch[c].append(Int(f[i + c])) }
        }
    }
    return ch.map { v in let s = v.sorted(); return Double(s[s.count / 2]) }
}

// ---- the run --------------------------------------------------------------

DispatchQueue.global().async {
    main_ {
        window.orderFrontRegardless()
    }
    sleep_(0.6)
    let wn = CGWindowID(window.windowNumber)
    var content: SCShareableContent?
    let sem = DispatchSemaphore(value: 0)
    SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) { c, e in
        if let e = e { die("SCShareableContent: \(e)", 4) }
        content = c; sem.signal()
    }
    sem.wait()
    guard let win = content?.windows.first(where: { $0.windowID == wn }) else {
        die("the probe's own window is not shareable (Screen Recording permission?)", 4)
    }
    let filter = SCContentFilter(desktopIndependentWindow: win)
    scale = max(1, Int(filter.pointPixelScale.rounded()))
    var titlebar = 0.0, contentH = 0.0, frameW = 0.0
    main_ {
        titlebar = window.frame.height - window.contentLayoutRect.height
        contentH = window.contentLayoutRect.height
        frameW = window.frame.width
    }
    let strip = 24.0
    let cfg = SCStreamConfiguration()
    cfg.sourceRect = CGRect(x: frameW - strip, y: titlebar, width: strip, height: contentH)
    cfg.width = Int(strip) * scale
    cfg.height = Int(contentH) * scale
    cfg.pixelFormat = kCVPixelFormatType_32BGRA
    cfg.minimumFrameInterval = CMTime(value: 1, timescale: 120)
    cfg.queueDepth = 8
    cfg.showsCursor = false
    let s = SCStream(filter: filter, configuration: cfg, delegate: cap)
    do { try s.addStreamOutput(cap, type: .screen, sampleHandlerQueue: DispatchQueue(label: "cap")) } catch {
        die("addStreamOutput: \(error)", 4)
    }
    let ss = DispatchSemaphore(value: 0)
    s.startCapture { e in if let e = e { die("startCapture: \(e)", 4) }; ss.signal() }
    ss.wait()
    stream = s

    var reduce = false, style = "?", scaleF = 0.0
    main_ {
        reduce = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        style = NSScroller.preferredScrollerStyle == .overlay ? "overlay" : "legacy"
        scaleF = window.backingScaleFactor
    }
    let showBars = UserDefaults.standard.string(forKey: "AppleShowScrollBars") ?? "(absent)"
    print("conditions: preferredScrollerStyle=\(style) AppleShowScrollBars=\(showBars) "
          + "reduceMotion=\(reduce) backingScale=\(scaleF) strip=\(Int(strip))pt x \(Int(contentH))pt "
          + "at \(scale)x, frame interval 1/120 s")
    // Layout: an overlay scroller lies over the content's right edge; a
    // legacy one is tiled beside it and the content loses its width.
    for (n, st) in [("overlay", NSScroller.Style.overlay), ("legacy", NSScroller.Style.legacy)] {
        let size = NSScrollView.contentSize(forFrameSize: contentSize, horizontalScrollerClass: nil,
            verticalScrollerClass: NSScroller.self, borderType: .noBorder, controlSize: .regular,
            scrollerStyle: st)
        print("layout: \(n) content width \(size.width)pt of a \(contentSize.width)pt frame")
    }
    let expanded = NSScroller.scrollerWidth(for: .regular, scrollerStyle: .overlay)
    let legacyW = NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy)
    print("api: scrollerWidth overlay=\(expanded)pt legacy=\(legacyW)pt")

    func setup(_ appearance: NSAppearance.Name, _ color: NSColor, _ st: NSScroller.Style) {
        main_ {
            window.appearance = NSAppearance(named: appearance)
            fresh(color, st)
        }
        sleep_(2.5)  // let anything the setup revealed fade
    }

    let flash = { scroll.flashScrollers() }
    let wheel = {
        let cg = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1,
                         wheel1: -3, wheel2: 0, wheel3: 0)!
        if let e = NSEvent(cgEvent: cg) { scroll.scrollWheel(with: e) }
    }
    let phased = {
        for (i, phase) in [1, 2, 2, 2, 4].enumerated() {
            let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
                             wheel1: phase == 4 ? 0 : -12, wheel2: 0, wheel3: 0)!
            cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
            cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: Int64(phase))
            _ = i
            if let e = NSEvent(cgEvent: cg) { scroll.scrollWheel(with: e) }
        }
    }
    // The document grows under a view that stays on its lines: a terminal
    // scrolled back while output arrives, whose window follows its lines up
    // the history. Neither the clip view's origin nor the visible content
    // moves; only the knob's proportion does.
    let grows = {
        doc.setFrameSize(NSSize(width: contentSize.width, height: doc.frame.height + 200))
        scroll.reflectScrolledClipView(scroll.contentView)
    }
    let programmatic = {
        doc.setFrameSize(NSSize(width: contentSize.width, height: doc.frame.height + 200))
        scroll.contentView.scroll(to: NSPoint(x: 0, y: doc.frame.height - scroll.contentView.bounds.height))
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    // Timing: dark appearance, black document.
    setup(.darkAqua, .black, .overlay)
    let flashes = trials("overlay, flashScrollers()", flash)
    setup(.darkAqua, .black, .overlay)
    _ = trials("overlay, one wheel click, in-process", wheel)
    setup(.darkAqua, .black, .overlay)
    _ = trials("overlay, a phased trackpad gesture, in-process", phased)
    setup(.darkAqua, .black, .overlay)
    _ = trials("overlay, programmatic scroll as the document grows", programmatic)
    setup(.darkAqua, .black, .overlay)
    _ = trials("overlay, the document grows under a view that stays", grows)

    // The fade's shape: one flash's frames from the last at the peak to the
    // first back at the baseline, as the fraction of the peak's ink.
    setup(.darkAqua, .black, .overlay)
    _ = trial(flash)
    sleep_(0.6)
    do {
        let baseline = cap.now()
        cap.start()
        sleep_(0.05)
        var t0: UInt64 = 0
        main_ { scroll.flashScrollers(); t0 = mach_absolute_time() }
        sleep_(1.6)
        let fs = cap.stop().filter { $0.ticks >= t0 }
        let inks = fs.map { ink($0.bytes, baseline) }
        if let peak = inks.max(), peak > 0 {
            let shape = zip(fs, inks).filter { ms($0.0.ticks - t0) > 900 }
                .map { String(format: "%.0f:%.2f", ms($0.0.ticks - t0), Double($0.1) / Double(peak)) }
            print("fade shape, ms:fraction of the peak: " + shape.joined(separator: " "))
        }
    }

    // Geometry and colour, read as a row profile rather than a thresholded
    // difference: a light knob over a white document has no contrast, so the
    // knob is located on whichever background shows it, and the same pixels
    // are read over both. The profile is the row through the knob's middle,
    // run-length encoded across the strip, right edge last.
    func profile(_ f: [UInt8], row y: Int) -> String {
        let (w, _) = cap.size()
        var runs: [(Int, [Int])] = []
        for x in 0..<w {
            let i = (y * w + x) * 4
            let c = [Int(f[i + 2]), Int(f[i + 1]), Int(f[i])]
            if let last = runs.last, zip(last.1, c).allSatisfy({ abs($0 - $1) <= 3 }) {
                runs[runs.count - 1].0 += 1
            } else {
                runs.append((1, c))
            }
        }
        return runs.map { "\($0.0)x(\($0.1[0]),\($0.1[1]),\($0.1[2]))" }.joined(separator: " ")
    }
    func pixel(_ f: [UInt8], _ x: Int, _ y: Int) -> [Double] {
        let (w, _) = cap.size()
        let i = (y * w + x) * 4
        return [Double(f[i + 2]), Double(f[i + 1]), Double(f[i])]
    }
    for (name, ap) in [("dark", NSAppearance.Name.darkAqua), ("light", NSAppearance.Name.aqua)] {
        var peaks: [NSColor: ([UInt8], [UInt8])] = [:]
        for bg in [NSColor.black, NSColor.white] {
            setup(ap, bg, .overlay)
            _ = trial(flash)
            sleep_(0.6)
            let b0 = cap.now()
            let t = trial(flash)
            peaks[bg] = (b0, t.peakFrame)
        }
        let boxes = [NSColor.black, NSColor.white].compactMap { bg in peaks[bg].flatMap { box($0.1, $0.0) } }
        guard let bx = boxes.max(by: { ($0.2 - $0.0) * ($0.3 - $0.1) < ($1.2 - $1.0) * ($1.3 - $1.1) }) else {
            print("overlay knob, \(name): NOT SEEN over black or white")
            continue
        }
        let (w, _) = cap.size()
        let cy = (bx.1 + bx.3) / 2, cx = (bx.0 + bx.2) / 2
        print(String(format: "overlay knob, %@: %d px wide, %d px from the right edge, %d px tall (at %dx)",
                     name, bx.2 - bx.0 + 1, w - 1 - bx.2, bx.3 - bx.1 + 1, scale))
        let over = [NSColor.black, NSColor.white].map { pixel(peaks[$0]!.1, cx, cy) }
        let a = (0..<3).map { 1 - (over[1][$0] - over[0][$0]) / 255 }
        let alpha = a.reduce(0, +) / 3
        let c = (0..<3).map { alpha > 0.01 ? over[0][$0] / alpha : 0 }
        print(String(format: "   centre over black RGB %.0f %.0f %.0f, over white %.0f %.0f %.0f "
                     + "-> alpha %.2f, colour RGB %.0f %.0f %.0f",
                     over[0][0], over[0][1], over[0][2], over[1][0], over[1][1], over[1][2],
                     alpha, c[0], c[1], c[2]))
        for bg in [NSColor.black, NSColor.white] {
            print("   row \(cy) over \(bg == .black ? "black" : "white"): \(profile(peaks[bg]!.1, row: cy))")
        }
    }

    // Hover cannot be driven in-process -- the scroll view exposes no
    // tracking area to send `mouseEntered` to, and posting a real move would
    // take the user's cursor -- so the legacy scroller, which is what an
    // overlay scroller widens into, is read instead: a track the width of
    // the scroller and a knob inside it, always drawn.
    let areas = scroll.trackingAreas.count + (scroll.verticalScroller?.trackingAreas.count ?? 0)
    print("hover: \(areas) tracking areas on the scroll view and its scroller; not driven")
    for (name, ap) in [("dark", NSAppearance.Name.darkAqua), ("light", NSAppearance.Name.aqua)] {
        for bg in [NSColor.black, NSColor.white] {
            setup(ap, bg, .legacy)
            main_ { scroll.needsDisplay = true; scroll.verticalScroller?.needsDisplay = true }
            sleep_(0.4)
            let f = cap.now()
            let (_, h) = cap.size()
            print("legacy, \(name), over \(bg == .black ? "black" : "white"):")
            print("   track row \(h / 8): \(profile(f, row: h / 8))")
            print("   knob row \(h / 2): \(profile(f, row: h / 2))")
        }
    }
    _ = flashes
    let st = DispatchSemaphore(value: 0)
    s.stopCapture { _ in st.signal() }
    _ = st.wait(timeout: .now() + 2)
    exit(0)
}

app.run()
