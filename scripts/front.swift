// The owner pid, name and bounds of the frontmost ordinary window (layer
// 0), so an input-posting check can refuse to act unless baud owns it.
// Build or run the way scripts/winid.swift documents.
import Foundation
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
    let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
    print("\(w["kCGWindowOwnerPID"] as? Int32 ?? -1) \(w["kCGWindowOwnerName"] as? String ?? "?") bounds=\(b["X"] ?? 0),\(b["Y"] ?? 0),\(b["Width"] ?? 0),\(b["Height"] ?? 0)")
    break
}
