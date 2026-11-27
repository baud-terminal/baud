// How long `NSScroller.preferredScrollerStyle` takes. baud asks it on every
// wheel event, to notice the "Show scroll bars" setting changing, so it has
// to be cheap. No window and no input: a class method timed in a loop.
//
// Build with the real Xcode toolchain (see tools/scripts/winid.swift for why):
//
//   DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//   /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o scroller-style-cost tools/scripts/scroller-style-cost.swift
//
//   ./scroller-style-cost          # the steady state
//   ./scroller-style-cost first    # the first call, once per process
import AppKit
import Foundation

// `first` times the process's first call alone, which is the one baud makes
// on the launch path in `App::new`; run it in several fresh processes. With
// no argument, the steady state a wheel event pays.
_ = NSApplication.shared
var sink = 0
if CommandLine.arguments.dropFirst().first == "first" {
    let t0 = DispatchTime.now().uptimeNanoseconds
    sink &+= NSScroller.preferredScrollerStyle.rawValue
    let dt = DispatchTime.now().uptimeNanoseconds - t0
    print("first call: \(Double(dt) / 1000.0) us (\(sink % 2))")
    exit(0)
}
for _ in 0..<1000 { sink &+= NSScroller.preferredScrollerStyle.rawValue }
for round in 0..<5 {
    let n = 100_000
    let t0 = DispatchTime.now().uptimeNanoseconds
    for _ in 0..<n { sink &+= NSScroller.preferredScrollerStyle.rawValue }
    let dt = DispatchTime.now().uptimeNanoseconds - t0
    print("round \(round): \(Double(dt) / Double(n)) ns a call")
}
print("style now: \(NSScroller.preferredScrollerStyle == .legacy ? "legacy" : "overlay") (\(sink % 2))")
