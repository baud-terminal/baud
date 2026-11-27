// The still of a trail clip: the frame in a span with the most trail on
// screen, written as a PNG (tools/scripts/trail-clips.sh runs it on the diagonal).
//
// It picks by content, not by clock. When the child moves the cursor, the
// time the move lands on film depends on how long the recorder took to find
// the window and the child to see GO, and the ghost is visible for a tenth of
// a second, so a fixed timestamp misses it. Instead:
//   - `before` is the frame on screen at <before-s>, the cursor at rest;
//   - the move is the first later frame that differs from it;
//   - `settled` is the last frame at or before <settled-s>, the trail gone;
//   - the still is the frame from the move on that differs most from
//     `settled`. From the move on the cursor is where `settled` has it, so
//     what differs is exactly the trail.
// A difference is the number of sampled pixels (every fourth, both ways)
// whose luma moved by more than 24 of 255. It prints that count for the
// still, and the driver requires it above a floor for every trail and under
// it for `off` -- the control that says it found a trail and not a cursor.
//
// Build the way tools/scripts/trail-record.swift says.
//
// usage: trail-still <clip.mp4> <out.png> <before-s> <settled-s> <scale>
import AVFoundation
import AppKit

func die(_ m: String, _ code: Int32) -> Never {
    FileHandle.standardError.write(("trail-still: " + m + "\n").data(using: .utf8)!)
    exit(code)
}

let a = CommandLine.arguments
guard a.count == 6, let before = Double(a[3]), let settled = Double(a[4]), let scale = Double(a[5]) else {
    die("usage: trail-still <clip.mp4> <out.png> <before-s> <settled-s> <scale>", 2)
}
let asset = AVURLAsset(url: URL(fileURLWithPath: a[1]))
let out = URL(fileURLWithPath: a[2])
let step = 4
let moved = 24

struct Frame { let t: CMTime; let luma: [UInt8] }

/// Every fourth pixel's luma, both ways.
func sample(_ pb: CVPixelBuffer) -> [UInt8] {
    CVPixelBufferLockBaseAddress(pb, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(pb, .readOnly) }
    let w = CVPixelBufferGetWidth(pb), h = CVPixelBufferGetHeight(pb)
    let row = CVPixelBufferGetBytesPerRow(pb)
    let base = CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)
    var l = [UInt8]()
    l.reserveCapacity((w / step) * (h / step))
    for y in stride(from: 0, to: h, by: step) {
        let p = base + y * row
        for x in stride(from: 0, to: w, by: step) {
            let b = Int(p[x * 4]), g = Int(p[x * 4 + 1]), r = Int(p[x * 4 + 2])
            l.append(UInt8((2 * r + 5 * g + b) / 8))
        }
    }
    return l
}

func differs(_ x: [UInt8], _ y: [UInt8]) -> Int {
    var n = 0
    for i in 0..<min(x.count, y.count) where abs(Int(x[i]) - Int(y[i])) > moved { n += 1 }
    return n
}

let sem = DispatchSemaphore(value: 0)
Task {
    do {
        guard let track = try await asset.loadTracks(withMediaType: .video).first else { die("no video track", 1) }
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(
            track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
        reader.add(output)
        guard reader.startReading() else { die("cannot read: \(reader.error?.localizedDescription ?? "?")", 1) }
        var atBefore: Frame?
        var span = [Frame]()
        while let sb = output.copyNextSampleBuffer() {
            guard let pb = CMSampleBufferGetImageBuffer(sb) else { continue }
            let t = CMSampleBufferGetPresentationTimeStamp(sb)
            if t.seconds <= before {
                atBefore = Frame(t: t, luma: sample(pb))
            } else if t.seconds <= settled {
                span.append(Frame(t: t, luma: sample(pb)))
            } else {
                break
            }
        }
        guard let rest = atBefore else { die("no frame at or before \(before) s", 1) }
        guard let end = span.last else { die("no frame between \(before) and \(settled) s", 1) }
        guard let move = span.firstIndex(where: { differs($0.luma, rest.luma) > 8 }) else {
            die("the cursor never moved between \(before) and \(settled) s", 1)
        }
        let best = span[move...].max { differs($0.luma, end.luma) < differs($1.luma, end.luma) }!
        let n = differs(best.luma, end.luma)

        let g = AVAssetImageGenerator(asset: asset)
        g.requestedTimeToleranceBefore = .zero
        g.requestedTimeToleranceAfter = .zero
        let (img, _) = try await g.image(at: best.t)
        let w = Int(Double(img.width) * scale), h = Int(Double(img.height) * scale)
        let ctx = CGContext(
            data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        ctx.interpolationQuality = .high
        ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let png = NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])
        else { die("cannot encode the still", 1) }
        try png.write(to: out)
        let ms = Int(((best.t.seconds - span[move].t.seconds) * 1000).rounded())
        print(String(format: "move at %.3f s, still at %.3f s (+%d ms), trail %d", span[move].t.seconds, best.t.seconds, ms, n))
    } catch {
        die("\(error)", 1)
    }
    sem.signal()
}
sem.wait()
