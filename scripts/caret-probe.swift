// AppKit's own text caret, measured: an NSTextInsertionIndicator in an
// offscreen borderless window under the accessory policy -- no input posted,
// the app never activates -- its animation tree dumped (key times, values,
// durations), then its presentation opacity sampled over two cycles. baud's
// caret blinks at the rate this reports (baud-app/src/caret.rs), switching
// hard at its mean opacity. Build with the real Xcode toolchain, as
// scripts/scroller-probe.swift says, and run the binary; pass
// `-NSTextInsertionPointBlinkPeriodOn 1000` and the like to see a set rate:
//
//   X=/Applications/Xcode.app/Contents/Developer
//   DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o caret-probe scripts/caret-probe.swift && ./caret-probe
import AppKit
import ObjectiveC
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
func dump(_ a: CAAnimation?, _ ind: String) {
    guard let a = a else { print(ind + "nil"); return }
    print(ind + "\(type(of: a)) duration=\(a.duration) repeat=\(a.repeatCount) begin=\(a.beginTime) toff=\(a.timeOffset)")
    if let k = a as? CAKeyframeAnimation {
        print(ind + "  keyPath=\(k.keyPath ?? "") calc=\(k.calculationMode.rawValue)")
        print(ind + "  keyTimes=\((k.keyTimes ?? []).map { $0.doubleValue })")
        print(ind + "  values=\(k.values ?? [])")
    }
    if let g = a as? CAAnimationGroup { for s in g.animations ?? [] { dump(s, ind + "    ") } }
}
func walk(_ l: CALayer, _ depth: Int) {
    let pad = String(repeating: " ", count: depth * 2)
    print(pad + "layer name=\(l.name ?? "-") keys=\(l.animationKeys() ?? [])")
    for k in l.animationKeys() ?? [] { dump(l.animation(forKey: k), pad + "  ") }
    for s in l.sublayers ?? [] { walk(s, depth + 1) }
}
let win = NSWindow(contentRect: NSRect(x: -20000, y: -20000, width: 200, height: 100), styleMask: [.borderless], backing: .buffered, defer: false)
let ind = NSTextInsertionIndicator(frame: NSRect(x: 10, y: 10, width: 2, height: 20))
ind.displayMode = .automatic
win.contentView!.addSubview(ind)
win.orderFrontRegardless()
RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.3))
print("NSTextInsertionPointBlinkPeriodOn default:", UserDefaults.standard.object(forKey: "NSTextInsertionPointBlinkPeriodOn") ?? "unset")
print("NSTextInsertionPointBlinkPeriodOff default:", UserDefaults.standard.object(forKey: "NSTextInsertionPointBlinkPeriodOff") ?? "unset")
if let l = ind.layer { walk(l, 0) }
let t0 = CACurrentMediaTime()
var last: Float = -1; var lastT = 0.0
var hist: [Float: Double] = [:]
var prevT = 0.0
while CACurrentMediaTime() - t0 < 2.0 {
    RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.001))
    let t = CACurrentMediaTime() - t0
    guard let pres = ind.layer?.presentation(), let sub = pres.sublayers?.first(where: { $0.name == "indicator" }) else { continue }
    let op = sub.opacity
    hist[op, default: 0] += t - prevT; prevT = t
    if op != last { print(String(format: "t=%7.1f ms opacity=%.2f (prev held %.1f ms)", t * 1000, op, (t - lastT) * 1000)); last = op; lastT = t }
}
print("time at each opacity over 2 s:", hist.sorted { $0.key < $1.key }.map { String(format: "%.2f:%.0fms", $0.key, $0.value * 1000) })
