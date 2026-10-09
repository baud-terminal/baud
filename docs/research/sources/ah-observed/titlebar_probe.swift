// titlebar_probe.swift -- where a trailing NSTitlebarAccessoryViewController
// lands in a window shaped like glassnote's titled chrome (Titled |
// FullSizeContentView | Closable | Miniaturizable | Resizable, transparent
// titlebar, hidden title). Research probe, not baud code.
// Build: swiftc -O titlebar_probe.swift -o titlebar_probe
import AppKit

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let mask: NSWindow.StyleMask = [.titled, .fullSizeContentView, .closable, .miniaturizable, .resizable]
let win = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 800, height: 500),
                   styleMask: mask, backing: .buffered, defer: false)
win.titlebarAppearsTransparent = true
win.titleVisibility = .hidden
for (name, attr) in [("trailing", NSLayoutConstraint.Attribute.trailing), ("right", .right)] {
    let vc = NSTitlebarAccessoryViewController()
    let v = NSView(frame: NSRect(x: 0, y: 0, width: 96, height: 10))
    vc.view = v
    vc.layoutAttribute = attr
    win.addTitlebarAccessoryViewController(vc)
    win.layoutIfNeeded()
    win.contentView?.superview?.layoutSubtreeIfNeeded()
    let inWin = v.convert(v.bounds, to: nil)
    print("\(name): accessory view frame in window \(inWin)")
    win.removeTitlebarAccessoryViewController(at: win.titlebarAccessoryViewControllers.count - 1)
}
let frame = win.frame
let clr = win.contentLayoutRect
print("window frame \(frame.size); contentLayoutRect \(clr); titlebar strip height \(frame.height - clr.maxY - 0)")
for b in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
    if let btn = win.standardWindowButton(b) {
        print("\(b.rawValue): \(btn.convert(btn.bounds, to: nil))")
    }
}
print("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
