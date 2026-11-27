// The control for the luma measurement under glass: a plain, non-glass
// window whose content is CALayer bands in the sRGB colour space, the same
// seven colours tools/scripts/luma-swatch.sh paints in baud. Run with the real
// toolchain, the way tools/scripts/winid.swift says; capture both windows with
// screencapture -x -o -l <id> and compare the median of each band.
import AppKit
let colours: [(CGFloat, CGFloat, CGFloat)] = [(255,0,0),(0,255,0),(0,0,255),(128,128,128),(255,255,255),(0,0,0),(200,100,50)]
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let frame = NSRect(x: 120, y: 120, width: 600, height: 420)
let win = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: false)
win.title = "swatch"
let view = NSView(frame: NSRect(x: 0, y: 0, width: 600, height: 420))
view.wantsLayer = true
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let bandH: CGFloat = 60
for (i, c) in colours.enumerated() {
    let l = CALayer()
    l.frame = CGRect(x: 0, y: CGFloat(i) * bandH, width: 600, height: bandH)
    l.backgroundColor = CGColor(colorSpace: space, components: [c.0/255, c.1/255, c.2/255, 1])!
    view.layer!.addSublayer(l)
}
win.contentView = view
win.makeKeyAndOrderFront(nil)
app.run()
