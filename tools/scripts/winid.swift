// The CoreGraphics window ids a process owns, with their bounds, so a
// window can be photographed without bringing it to the front:
//
//   screencapture -x -o -l <id> out.png
//
// Build with the real Xcode toolchain; the `swiftc` on this box's path is
// the xcode-select shim pointing at a nix SDK that carries no compiler:
//
//   DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//   /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o winid tools/scripts/winid.swift
//
// usage: winid <pid>
import CoreGraphics
import Foundation
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in list where (w["kCGWindowOwnerPID"] as? Int32) == pid {
    let n = w["kCGWindowNumber"] as? Int ?? -1
    let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
    let on = w["kCGWindowIsOnscreen"] as? Bool ?? false
    print("\(n) onscreen=\(on) bounds=\(b["X"] ?? 0),\(b["Y"] ?? 0),\(b["Width"] ?? 0),\(b["Height"] ?? 0)")
}
