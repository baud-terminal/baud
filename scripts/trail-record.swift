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
//   - baud's window has to be frontmost when the capture starts -- it waits
//     up to 2 s for that -- and when it stops, or it exits 3. An occluded
//     window was recorded once and gave a thin clip with no error, which is
//     the probe that cannot fire.
//   - the writer has to finish completed, with at least <min-frames>. Only
//     a changed window sends a frame, so the six moves with no trail leave
//     about 10, the ghost's six tenths of a second about 65 and the others
//     hundreds: the driver asks 30 of a trail and 1 of its control.
//
// Build (the `swiftc` on this box's path is the xcode-select shim pointing at
// a nix SDK with no compiler):
//   X=/Applications/Xcode.app/Contents/Developer
//   DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o trail-record scripts/trail-record.swift
//
// usage: trail-record <pid> <out.mp4> <go-file> <done-file> <tail-seconds> <min-frames>
import AVFoundation
import AppKit
import CoreMedia
import ScreenCaptureKit

_ = NSApplication.shared  // ScreenCaptureKit asserts without a WindowServer connection

func die(_ m: String, _ code: Int32) -> Never {
    FileHandle.standardError.write(("trail-record: " + m + "\n").data(using: .utf8)!)
    exit(code)
}

let a = CommandLine.arguments
guard a.count == 7, let pid = pid_t(a[1]), let tail = Double(a[5]), let frameFloor = Int(a[6]) else {
    die("usage: trail-record <pid> <out.mp4> <go-file> <done-file> <tail-seconds> <min-frames>", 2)
}
let out = URL(fileURLWithPath: a[2])
let goFile = a[3], doneFile = a[4]

/// The pid owning the frontmost ordinary (layer 0) window, as
/// scripts/front.swift reads it.
func frontPid() -> pid_t {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
        return w["kCGWindowOwnerPID"] as? pid_t ?? -1
    }
    return -1
}

final class Rec: NSObject, SCStreamOutput {
    var writer: AVAssetWriter?
    var input: AVAssetWriterInput?
    var adaptor: AVAssetWriterInputPixelBufferAdaptor?
    var started = false
    var frames = 0
    let q = DispatchQueue(label: "trail-record")

    func setup(w: Int, h: Int) throws {
        try? FileManager.default.removeItem(at: out)
        let wr = try AVAssetWriter(outputURL: out, fileType: .mp4)
        let settings: [String: Any] = [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: w, AVVideoHeightKey: h,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 6_000_000,
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

    func stream(_ s: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sb.isValid,
              let att = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let raw = att.first?[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
              let pb = CMSampleBufferGetImageBuffer(sb),
              let wr = writer, let inp = input else { return }
        let t = CMSampleBufferGetPresentationTimeStamp(sb)
        if !started {
            wr.startWriting()
            wr.startSession(atSourceTime: t)
            started = true
        }
        if inp.isReadyForMoreMediaData, adaptor!.append(pb, withPresentationTime: t) { frames += 1 }
    }
}

let rec = Rec()
let sem = DispatchSemaphore(value: 0)
Task {
    do {
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
        try rec.setup(w: cfg.width, h: cfg.height)
        let s = SCStream(filter: filter, configuration: cfg, delegate: nil)
        try s.addStreamOutput(rec, type: .screen, sampleHandlerQueue: rec.q)
        try await s.startCapture()
        print("window \(w.frame) at \(scale)x -> \(cfg.width)x\(cfg.height)")
        try await Task.sleep(nanoseconds: 400_000_000)
        FileManager.default.createFile(atPath: goFile, contents: nil)
        while !FileManager.default.fileExists(atPath: doneFile) {
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        try await Task.sleep(nanoseconds: UInt64(tail * 1e9))
        let endFront = frontPid()
        try await s.stopCapture()
        rec.q.sync { rec.input?.markAsFinished() }
        await rec.writer?.finishWriting()
        let status = rec.writer?.status ?? .unknown
        print("frames \(rec.frames), writer status \(status.rawValue)")
        guard endFront == pid else { die("pid \(pid) is not frontmost at the stop (pid \(endFront) is)", 3) }
        guard status == .completed else {
            die("the writer did not complete: \(rec.writer?.error?.localizedDescription ?? "no error")", 4)
        }
        guard rec.frames >= frameFloor else { die("\(rec.frames) frames, under the floor of \(frameFloor)", 4) }
    } catch {
        die("\(error)", 1)
    }
    sem.signal()
}
sem.wait()
