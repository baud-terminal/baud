// Record one process's window to an H.264 MP4 through ScreenCaptureKit, for
// the trail clips in docs/cursor/ (scripts/trail-clips.sh drives it).
//
// It captures the window alone (SCContentFilter desktopIndependentWindow) at
// its backing scale and at most 120 frames a second, keeps only the frames
// the window server marks complete -- an idle window sends none, so the file
// is variable frame rate -- and writes each at its own presentation time. It
// creates GO 0.4 s after the capture starts, so the child's first move is on
// film, and stops `tail` seconds after the child creates DONE.
//
// It refuses rather than record a clip nobody should use:
//   - in the window form, baud's window has to be frontmost when the
//     capture starts -- it waits up to 2 s for that -- and in both forms
//     when it stops, or it exits 3 (the display form starts before baud and
//     checks at the stop). An occluded window was recorded once and gave a
//     thin clip with no error, which is the probe that cannot fire.
//   - the writer has to finish completed, with at least <min-frames>. Only
//     a changed window sends a frame, so the six moves with no trail leave
//     about 10, the ghost's six tenths of a second about 65 and the others
//     hundreds: the driver asks 30 of a trail and 1 of its control.
//   - the writer may drop no more than 1% of the complete frames the capture
//     delivered, or it exits 5: a frame dropped because the encoder was
//     behind is a stutter the trail did not make. On a clip under 100 frames
//     -- the ghost's 64, the control's 10 -- that is no drop at all. The committed clips before
//     this check hold 120 Hz frame gaps and 60 Hz ones about 4:3, on every
//     trail, and nothing said which were drops. It prints what the capture
//     sent by status beside what the writer took, so a capture that sent
//     fewer frames is told from a writer that lost them.
//
// The README's GIF uses a second form, display (baud-app/examples/demo.rs,
// --record). A window can be filmed only once it exists, and in a take of
// the README session baud's window was already on screen when a window
// capture found it, 24 ms after it started, and the capture ran 205 ms
// later, so the launch splash had begun before the film. The display form
// starts before baud does: it films the main display at its backing scale,
// every application running when it starts left out, at most 60 frames a
// second (a 3456x2234 display at 120 passes H.264 level 5.2's macroblock
// rate), at <bit/s>, and creates READY once the capture runs; the
// caller starts baud after READY and crops the film to baud's window. It
// prints the display's size and scale, reads baud's pid from PID-FILE at the
// stop for the frontmost check, and ends the film at the stop: the capture
// sends no frame while nothing on the display changes, so the last frame is
// written again at the stop's time, as long after the last frame's time as
// the stop came after that frame arrived, the stop read once the capture
// has stopped, and the session is ended there. Without that end the MP4
// gives its last sample the gap before it as its duration, the whole still
// tail again, and the film plays past the stop by as long. A frame that
// came at the stop ends the film at its own time. It prints how long the
// film runs, from its first frame to the stop; a gap past 30 s, or a frame
// the writer would not take again, is refused (exit 6).
//
// Built with -D WRITER_CHECK, it is the writer alone, with no capture and
// no window: it writes <frames> frames 1/60 s apart through the same
// writer and the same end, the stop <hold> s after the last, so the demo's
// --dry --record can check that the film ends at the stop without filming.
//
// Build (the `swiftc` on this box's path is the xcode-select shim pointing at
// a nix SDK with no compiler):
//   X=/Applications/Xcode.app/Contents/Developer
//   DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o trail-record scripts/trail-record.swift
//
// usage: trail-record <pid> <out.mp4> <go-file> <done-file> <tail-seconds> <min-frames>
//        trail-record display <pid-file> <out.mp4> <ready-file> <done-file> <tail-seconds> <min-frames> <bit/s>
//        (-D WRITER_CHECK) trail-record <out.mp4> <frames> <hold-seconds> <bit/s>
import AVFoundation
import CoreMedia
#if !WRITER_CHECK
import AppKit
import ScreenCaptureKit
#endif

func die(_ m: String, _ code: Int32) -> Never {
    FileHandle.standardError.write(("trail-record: " + m + "\n").data(using: .utf8)!)
    exit(code)
}

final class Rec: NSObject {
    var writer: AVAssetWriter?
    var input: AVAssetWriterInput?
    var adaptor: AVAssetWriterInputPixelBufferAdaptor?
    var started = false
    var frames = 0
    /// Complete frames the capture delivered, and those the writer took
    /// none of: not ready for more (its encoder was behind) or refused.
    var delivered = 0
    var notReady = 0
    var refused = 0
    /// Every frame the capture sent, by its status, so a capture that sent
    /// fewer complete frames is told from a writer that dropped them.
    var statuses: [Int: Int] = [:]
    /// In the display form, the session's start, and the last frame
    /// written, its time and the host clock when it arrived, to end the
    /// film at the stop.
    var holdsLast = false
    var start: CMTime?
    var last: (CVPixelBuffer, CMTime, CMTime)?
    /// What the end found: how long the last frame was held to the stop,
    /// and the film's length from its first frame to the stop -- nil when
    /// the last frame was not written again.
    var held: Double?
    var film: Double?
    let q = DispatchQueue(label: "trail-record")

    func setup(out: URL, w: Int, h: Int, bitrate: Int) throws {
        try? FileManager.default.removeItem(at: out)
        let wr = try AVAssetWriter(outputURL: out, fileType: .mp4)
        let settings: [String: Any] = [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: w, AVVideoHeightKey: h,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: bitrate,
                AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
            ],
        ]
        let inp = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
        inp.expectsMediaDataInRealTime = true
        adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: inp, sourcePixelBufferAttributes: nil)
        wr.add(inp)
        writer = wr
        input = inp
    }

    /// One complete frame, at its presentation time `t`, `arrived` on the
    /// host clock; the session starts at the first.
    func take(_ pb: CVPixelBuffer, at t: CMTime, arrived: CMTime) {
        guard let wr = writer, let inp = input else { return }
        delivered += 1
        if !started {
            wr.startWriting()
            wr.startSession(atSourceTime: t)
            start = t
            started = true
        }
        if !inp.isReadyForMoreMediaData {
            notReady += 1
        } else if adaptor!.append(pb, withPresentationTime: t) {
            frames += 1
            if holdsLast {
                if last == nil { print("first frame") }
                last = (pb, t, arrived)
            }
        } else {
            refused += 1
        }
    }

    /// **The film ended at the stop**, `stop` on the host clock, once no
    /// frame can come: the last frame written again as long after its own
    /// time as the stop came after it arrived. A frame that arrived at the
    /// stop, or after it while the queue drained, already ends the film
    /// there. On `q` while a capture runs.
    func end(at stop: CMTime) {
        guard holdsLast, let l = last, let inp = input, let s = start else { return }
        let since = CMTimeSubtract(stop, l.2)
        let h = CMTimeGetSeconds(since)
        held = h
        var end = l.1
        if h > 0 {
            guard h <= 30 else { return }
            var waited = 0
            while !inp.isReadyForMoreMediaData && waited < 100 {
                usleep(10_000)
                waited += 1
            }
            end = CMTimeAdd(l.1, since)
            guard inp.isReadyForMoreMediaData, adaptor!.append(l.0, withPresentationTime: end) else { return }
            frames += 1
        }
        writer?.endSession(atSourceTime: end)
        film = CMTimeGetSeconds(CMTimeSubtract(end, s))
    }

    /// What the writer took, beside what it was given.
    func tally() {
        let status = writer?.status ?? .unknown
        let sent = statuses.sorted { $0.key < $1.key }.map { "\($0.key):\($0.value)" }.joined(separator: " ")
        print("frames \(frames) of \(delivered) complete, dropped \(notReady) not ready and \(refused) refused; sent by status \(sent); writer status \(status.rawValue)")
    }

    /// The end's two lines, or exit 6 when the last frame was not written
    /// again.
    func ended() {
        guard let h = held, let f = film else {
            die("the last frame was not written again at the stop (\(held.map { String(format: "%.3f s", $0) } ?? "no frame") after it)", 6)
        }
        print(h > 0 ? String(format: "held the last frame %.3f s, to the stop", h) : "the last frame came at the stop")
        print(String(format: "film %.6f s, from the first frame to the stop", f))
    }
}

let rec = Rec()

#if WRITER_CHECK
let a = CommandLine.arguments
guard a.count == 5, let n = Int(a[2]), n > 1, let hold = Double(a[3]), hold >= 0, let bitrate = Int(a[4]), bitrate > 0 else {
    die("usage: trail-record <out.mp4> <frames> <hold-seconds> <bit/s> (built with -D WRITER_CHECK)", 2)
}
setbuf(stdout, nil)
/// Frame `i`: 640x400, a shade of its own, so no two frames are alike.
func frame(_ i: Int) -> CVPixelBuffer {
    var made: CVPixelBuffer?
    let attrs = [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary
    guard CVPixelBufferCreate(nil, 640, 400, kCVPixelFormatType_32BGRA, attrs, &made) == kCVReturnSuccess,
          let pb = made else { die("no pixel buffer", 1) }
    CVPixelBufferLockBaseAddress(pb, [])
    let p = CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)
    for k in 0..<CVPixelBufferGetDataSize(pb) { p[k] = UInt8((i * 8 + k / 4096) & 0xff) }
    CVPixelBufferUnlockBaseAddress(pb, [])
    return pb
}
do {
    try rec.setup(out: URL(fileURLWithPath: a[1]), w: 640, h: 400, bitrate: bitrate)
} catch {
    die("\(error)", 1)
}
rec.holdsLast = true
// Each frame arrives at its own time, as if the host clock were the film's,
// from an origin off the writer's 1/600 s grid, as a capture's is.
let origin = CMTime(value: 1_000_012_345_678, timescale: 1_000_000_000)
for i in 0..<n {
    let t = CMTimeAdd(origin, CMTime(value: CMTimeValue(i), timescale: 60))
    while rec.started && rec.input?.isReadyForMoreMediaData == false { usleep(1_000) }
    rec.take(frame(i), at: t, arrived: t)
}
guard let lastArrived = rec.last?.2 else { die("the writer took no frame", 4) }
rec.end(at: CMTimeAdd(lastArrived, CMTime(seconds: hold, preferredTimescale: 1_000_000_000)))
rec.input?.markAsFinished()
let finished = DispatchSemaphore(value: 0)
rec.writer?.finishWriting { finished.signal() }
finished.wait()
rec.tally()
guard rec.writer?.status == .completed else {
    die("the writer did not complete: \(rec.writer?.error?.localizedDescription ?? "no error")", 4)
}
rec.ended()
#else
_ = NSApplication.shared  // ScreenCaptureKit asserts without a WindowServer connection

/// What is filmed: one process's window, or the display before baud starts.
enum Mode {
    case window(pid: pid_t, goFile: String)
    case display(pidFile: String, readyFile: String, bitrate: Int)
}

let usage = "usage: trail-record <pid> <out.mp4> <go-file> <done-file> <tail-seconds> <min-frames>\n"
    + "       trail-record display <pid-file> <out.mp4> <ready-file> <done-file> <tail-seconds> <min-frames> <bit/s>"
let a = CommandLine.arguments
let mode: Mode
let out: URL
let doneFile: String
let tail: Double
let frameFloor: Int
if a.count == 9, a[1] == "display", let t = Double(a[6]), let f = Int(a[7]), let b = Int(a[8]), b > 0 {
    mode = .display(pidFile: a[2], readyFile: a[4], bitrate: b)
    out = URL(fileURLWithPath: a[3])
    doneFile = a[5]
    tail = t
    frameFloor = f
    setbuf(stdout, nil)  // each line reaches the caller as it is printed
} else if a.count == 7, let p = pid_t(a[1]), let t = Double(a[5]), let f = Int(a[6]) {
    mode = .window(pid: p, goFile: a[3])
    out = URL(fileURLWithPath: a[2])
    doneFile = a[4]
    tail = t
    frameFloor = f
} else {
    die(usage, 2)
}

/// The pid owning the frontmost ordinary (layer 0) window, as
/// scripts/front.swift reads it.
func frontPid() -> pid_t {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
        return w["kCGWindowOwnerPID"] as? pid_t ?? -1
    }
    return -1
}

extension Rec: SCStreamOutput {
    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sb.isValid,
              let att = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let raw = att.first?[.status] as? Int else { return }
        statuses[raw, default: 0] += 1
        guard SCFrameStatus(rawValue: raw) == .complete,
              let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        take(pb, at: CMSampleBufferGetPresentationTimeStamp(sb), arrived: CMClockGetTime(CMClockGetHostTimeClock()))
    }
}

let sem = DispatchSemaphore(value: 0)

/// The pid a file names, or -1.
func pidIn(_ file: String) -> pid_t {
    guard let s = try? String(contentsOfFile: file, encoding: .utf8) else { return -1 }
    return pid_t(s.trimmingCharacters(in: .whitespacesAndNewlines)) ?? -1
}

Task {
    do {
        let s: SCStream
        switch mode {
        case .window(let pid, let goFile):
            var win: SCWindow?
            for _ in 0..<100 {
                let c = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
                win = c.windows
                    .filter { $0.owningApplication?.processID == pid && $0.frame.width > 300 && $0.frame.height > 200 }
                    .max { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
                if win != nil { break }
                try await Task.sleep(nanoseconds: 100_000_000)
            }
            guard let w = win else { die("no window of pid \(pid)", 3) }
            var front = frontPid()
            for _ in 0..<20 where front != pid {
                try await Task.sleep(nanoseconds: 100_000_000)
                front = frontPid()
            }
            guard front == pid else { die("pid \(pid) is not frontmost at the start (pid \(front) is)", 3) }
            let filter = SCContentFilter(desktopIndependentWindow: w)
            let cfg = SCStreamConfiguration()
            let scale = CGFloat(filter.pointPixelScale)
            cfg.width = Int(w.frame.width * scale) & ~1
            cfg.height = Int(w.frame.height * scale) & ~1
            cfg.minimumFrameInterval = CMTime(value: 1, timescale: 120)
            cfg.showsCursor = false
            cfg.pixelFormat = kCVPixelFormatType_32BGRA
            cfg.ignoreShadowsSingleWindow = true
            cfg.queueDepth = 8
            try rec.setup(out: out, w: cfg.width, h: cfg.height, bitrate: 6_000_000)
            s = SCStream(filter: filter, configuration: cfg, delegate: nil)
            try s.addStreamOutput(rec, type: .screen, sampleHandlerQueue: rec.q)
            try await s.startCapture()
            print("window \(w.frame) at \(scale)x -> \(cfg.width)x\(cfg.height)")
            try await Task.sleep(nanoseconds: 400_000_000)
            FileManager.default.createFile(atPath: goFile, contents: nil)
        case .display(_, let readyFile, let bitrate):
            let c = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
            guard let d = c.displays.first(where: { $0.displayID == CGMainDisplayID() }) else {
                die("no main display", 3)
            }
            let filter = SCContentFilter(display: d, excludingApplications: c.applications, exceptingWindows: [])
            let cfg = SCStreamConfiguration()
            let scale = CGFloat(filter.pointPixelScale)
            cfg.width = Int(CGFloat(d.width) * scale) & ~1
            cfg.height = Int(CGFloat(d.height) * scale) & ~1
            cfg.minimumFrameInterval = CMTime(value: 1, timescale: 60)
            cfg.showsCursor = false
            cfg.pixelFormat = kCVPixelFormatType_32BGRA
            if #available(macOS 14.0, *) { cfg.ignoreShadowsDisplay = true }
            cfg.queueDepth = 8
            try rec.setup(out: out, w: cfg.width, h: cfg.height, bitrate: bitrate)
            rec.holdsLast = true
            s = SCStream(filter: filter, configuration: cfg, delegate: nil)
            try s.addStreamOutput(rec, type: .screen, sampleHandlerQueue: rec.q)
            try await s.startCapture()
            print("display \(d.width)x\(d.height) pt at \(scale)x: \(cfg.width)x\(cfg.height) px, \(c.applications.count) applications left out")
            FileManager.default.createFile(atPath: readyFile, contents: nil)
        }
        while !FileManager.default.fileExists(atPath: doneFile) {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        try await Task.sleep(nanoseconds: UInt64(tail * 1e9))
        let endFront = frontPid()
        try await s.stopCapture()
        let stopHost = CMClockGetTime(CMClockGetHostTimeClock())
        rec.q.sync {
            rec.end(at: stopHost)
            rec.input?.markAsFinished()
        }
        await rec.writer?.finishWriting()
        let status = rec.writer?.status ?? .unknown
        rec.tally()
        let pid: pid_t
        switch mode {
        case .window(let p, _): pid = p
        case .display(let pidFile, _, _): pid = pidIn(pidFile)
        }
        guard endFront == pid else { die("pid \(pid) is not frontmost at the stop (pid \(endFront) is)", 3) }
        guard status == .completed else {
            die("the writer did not complete: \(rec.writer?.error?.localizedDescription ?? "no error")", 4)
        }
        if rec.holdsLast { rec.ended() }
        guard rec.frames >= frameFloor else { die("\(rec.frames) frames, under the floor of \(frameFloor)", 4) }
        // A frame the writer drops is a stutter in the clip that the trail
        // did not make: 5, which the driver retries.
        guard rec.notReady + rec.refused <= rec.delivered / 100 else {
            die("the writer dropped \(rec.notReady + rec.refused) of \(rec.delivered) frames", 5)
        }
    } catch {
        die("\(error)", 1)
    }
    sem.signal()
}
sem.wait()
#endif
