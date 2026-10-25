// Prototype of the ICE trail's fragment, written as the Rust reference would port it.
// f32 throughout. Renders PNGs and runs the completeness and reach checks.
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

typealias F = Float
struct P { var x: F; var y: F }
func +(a: P, b: P) -> P { P(x: a.x + b.x, y: a.y + b.y) }
func -(a: P, b: P) -> P { P(x: a.x - b.x, y: a.y - b.y) }
func *(a: P, s: F) -> P { P(x: a.x * s, y: a.y * s) }
func dot(_ a: P, _ b: P) -> F { a.x * b.x + a.y * b.y }
func len(_ a: P) -> F { (a.x * a.x + a.y * a.y).squareRoot() }
func clampf(_ v: F, _ a: F, _ b: F) -> F { min(max(v, a), b) }
func sat(_ v: F) -> F { clampf(v, 0, 1) }
func smoothstep(_ e0: F, _ e1: F, _ x: F) -> F { let t = sat((x - e0) / (e1 - e0)); return t * t * (3 - 2 * t) }

// ---- the shared hash and noise (fx_hash, fx_unit, fx_lattice, fx_noise)
func fxHash(_ v: UInt32) -> UInt32 {
    let s = v &* 747796405 &+ 2891336453
    let w = ((s >> ((s >> 28) &+ 4)) ^ s) &* 277803737
    return (w >> 22) ^ w
}
func fxUnit(_ h: UInt32) -> F { F(bitPattern: (h >> 9) | 0x3f800000) - 1 }
func fxLattice(_ x: Int32, _ y: Int32, _ seed: UInt32) -> F {
    fxUnit(fxHash(UInt32(bitPattern: x) &+ fxHash(UInt32(bitPattern: y) &+ seed)))
}
func fxNoise(_ p: P, _ seed: UInt32) -> F {
    let fx = p.x.rounded(.down), fy = p.y.rounded(.down)
    let ix = Int32(fx), iy = Int32(fy)
    let x = p.x - fx, y = p.y - fy
    let fade = { (v: F) -> F in v * v * v * (v * (v * 6 - 15) + 10) }
    let wx = fade(x), wy = fade(y)
    let a = fxLattice(ix, iy, seed), b = fxLattice(ix + 1, iy, seed)
    let c = fxLattice(ix, iy + 1, seed), d = fxLattice(ix + 1, iy + 1, seed)
    let ab = a + (b - a) * wx, cd = c + (d - c) * wx
    return ab + (cd - ab) * wy
}

// ---- the quad (trail::Segment), its rates as note W, section 8.4, derives them
struct Rect { var l, t, r, b: F
    var center: P { P(x: (l + r) / 2, y: (t + b) / 2) }
    var corners: [P] { [P(x: r, y: t), P(x: r, y: b), P(x: l, y: b), P(x: l, y: t)] }
}
struct Segment { var from: [P]; var to: Rect; var tau: [F]
    init(_ from: [P], _ to: Rect) {
        self.from = from; self.to = to
        let tg = to.corners, c = to.center
        let leads: [F?] = (0..<4).map { i in
            let dx = tg[i].x - from[i].x, dy = tg[i].y - from[i].y
            let ox = tg[i].x - c.x, oy = tg[i].y - c.y
            let travel = (dx * dx + dy * dy).squareRoot(), out = (ox * ox + oy * oy).squareRoot()
            return travel >= 1e-3 && out > 0 ? (dx * ox + dy * oy) / (travel * out) : nil }
        let live = leads.compactMap { $0 }
        let lo = live.min() ?? 0, hi = live.max() ?? 0
        tau = leads.map { l in
            guard let l = l, hi - lo > F.ulpOfOne else { return 0.4 }
            let s = (l - lo) / (hi - lo); return 0.1 * s + 0.4 * (1 - s) }
    }
    func at(_ t: F) -> [P] { let tg = to.corners
        return (0..<4).map { i in let k = F(exp2(Double(-10 * max(t, 0) / tau[i])))
            return P(x: tg[i].x + (from[i].x - tg[i].x) * k, y: tg[i].y + (from[i].y - tg[i].y) * k) } }
}
func quadDistance(_ p: P, _ q: [P]) -> F {
    var d = F.infinity; var w = 0
    for i in 0..<4 { let a = q[i], b = q[(i + 1) & 3], ba = b - a, pa = p - a
        let h = clampf(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0, 1)
        d = min(d, len(pa - ba * h))
        let side = ba.x * pa.y - ba.y * pa.x
        if a.y <= p.y { if b.y > p.y && side > 0 { w += 1 } } else if b.y <= p.y && side < 0 { w -= 1 } }
    return w != 0 ? -d : d
}

// ---- ICE: the numbers (k[0..8]) and the shape constants
let SPACING: F = 0.70, ARM: F = 0.62, WIDTH: F = 0.028, BRANCH: F = 0.21
let GROW: F = 0.12, HOLD: F = 0.25, SPREAD: F = 0.20, MELT: F = 0.22
let REACH: F = 1.15
let LEAD: F = 0.01          // kitty's TAU_FAST / 10: t_dep(u) = -LEAD * log2(1 - 0.999 u)
let ICE_SALT: UInt32 = 0x1CE0_1CE0
let RIME_SALT: UInt32 = 0x51ED_0000

struct Ice { var tail: P; var dir: P; var len: F; var unit: F; var age: F; var seed: UInt32; var quad: [P]; var hole: Rect }

func depTime(_ u: F) -> F { -LEAD * log2(max(1 - 0.999 * u, 1e-3)) }

// One crystal's opacity at `rel` (units, relative to the tail), for seed index k.
func crystal(_ k: Int32, _ rel: P, _ f: Ice, _ lu: F, _ perp: P, _ W: F, _ aa: F, _ sbase: UInt32) -> F {
    let h1 = fxHash(UInt32(bitPattern: k) &+ sbase)
    let h2 = fxHash(h1), h3 = fxHash(h2), h4 = fxHash(h3), h5 = fxHash(h4), h6 = fxHash(h5)
    let along = (F(k) + (fxUnit(h1) - 0.5) * 0.5) * SPACING
    let across = (fxUnit(h2) - 0.5) * 0.7
    let win = smoothstep(-0.25, 0, along) * (1 - smoothstep(lu, lu + 0.25, along))
    if win <= 0 { return 0 }
    let u = sat(along / lu)
    let delay = 0.03 * fxUnit(h5)
    let x = sat((f.age - depTime(u) - delay) / GROW)
    let grow = 1 - (1 - x) * (1 - x) * (1 - x)
    let m = HOLD + SPREAD * u + delay
    let fade = 1 - smoothstep(m, m + MELT, f.age)
    let L = ARM * (0.45 + 0.55 * fxUnit(h4)) * grow * (0.35 + 0.65 * fade)
    let exist = sat(L / (2 * W))
    if exist <= 0 || fade <= 0 { return 0 }
    let centre = f.dir * along + perp * across
    let r = rel - centre
    // rotation by a hashed angle in [0, 60) through tan(phi/2): no trig
    let s = 0.57735027 * fxUnit(h3)
    let den = 1 + s * s
    let cs = (1 - s * s) / den, sn = 2 * s / den
    var w = P(x: cs * r.x + sn * r.y, y: -sn * r.x + cs * r.y)
    // D6 fold into the wedge [0, 30] degrees, arm along +x
    w = P(x: abs(w.x), y: abs(w.y))
    let nA = P(x: -0.8660254, y: 0.5), nB = P(x: -0.5, y: 0.8660254)
    w = w - nA * (2 * max(dot(w, nA), 0))
    w = w - nB * (2 * max(dot(w, nB), 0))
    let line = { (d: F, wd: F) -> F in 1 - smoothstep(wd - aa, wd + aa, d) }
    // main arm
    let dm = len(w - P(x: clampf(w.x, 0, L), y: 0))
    let wm = W * (1.4 - 0.7 * sat(w.x / max(L, 1e-4)))
    let mainA = 0.95 * line(dm, wm)
    // a side branch at 60 degrees, rooted at the nearest j*B, stopped on the 30-degree mirror
    let e = P(x: 0.5, y: 0.8660254)
    let xr = w.x - w.y * 0.57735027
    let j = max((xr / BRANCH + 0.5).rounded(.down), 1)
    let xj = j * BRANCH
    let lj = min(0.6 * (L - xj), xj)
    let v = w - P(x: xj, y: 0)
    let hh = clampf(dot(v, e), 0, max(lj, 0))
    let ds = len(v - e * hh)
    let sideA = 0.60 * line(ds, W) * sat(lj / (2 * W))
    // the hexagonal plate at the centre, vertices on the arms
    let pr = 0.10 + 0.30 * fxUnit(h6) * fxUnit(h6)
    let dp = dot(w, P(x: 0.8660254, y: 0.5)) - pr * L * 0.8660254
    let plateA = 0.30 * (1 - smoothstep(-aa, aa, dp))
    return max(mainA, max(sideA, plateA)) * exist * fade * win
}

// The band fragment: opacity in [0, 1] at frame pixel p.
func iceAlpha(_ p: P, _ f: Ice) -> F {
    if p.x >= f.hole.l && p.x < f.hole.r && p.y >= f.hole.t && p.y < f.hole.b { return 0 }
    let c = f.unit
    let lu = f.len / c
    let rel = (p - f.tail) * (1 / c)
    let perp = P(x: -f.dir.y, y: f.dir.x)
    let along = dot(rel, f.dir)
    let dseg = len(rel - f.dir * clampf(along, 0, lu))
    let dq = quadDistance(p, f.quad)
    if dseg > REACH && dq / c > REACH { return 0 }
    // the pane: kitty's quad, body and a 1-px rim, fading over 0.6 HOLD
    let pa = sat(1 - f.age / (0.6 * HOLD))
    let pane = max(0.14 * (1 - smoothstep(-0.5, 0.5, dq)), 0.5 * (1 - smoothstep(0.5, 1.5, abs(dq)))) * pa
    if dseg > REACH { return pane }
    let W = max(WIDTH, 0.75 / c), aa = 0.5 / c
    let sbase = fxHash(f.seed &+ ICE_SALT)
    let k0 = (along / SPACING + 0.5).rounded(.down)
    var frost: F = 0
    for dk in -1...1 { frost = max(frost, crystal(Int32(k0) + Int32(dk), rel, f, lu, perp, W, aa, sbase)) }
    // the glint: one pass tail -> head over [0.4 HOLD, HOLD]
    let gp = (f.age - 0.4 * HOLD) / (0.6 * HOLD) * (lu + 2 * SPACING) - SPACING
    let gl = 1 - smoothstep(0, SPACING, abs(along - gp))
    frost = sat(frost * (1 + 0.3 * gl))
    // the rime
    let u = sat(along / lu)
    let front = (lu + 0.6) * (1 - exp2(-f.age / LEAD)) - 0.3
    let dep = 1 - smoothstep(front - 0.3, front + 0.3, along)
    let mr = HOLD + SPREAD * u
    let rfade = 1 - smoothstep(mr, mr + MELT, f.age)
    let s2 = f.seed ^ RIME_SALT
    let q2 = P(x: 0.8 * rel.x - 0.6 * rel.y, y: 0.6 * rel.x + 0.8 * rel.y)
    let n = 0.65 * fxNoise(rel * 2.3, s2) + 0.35 * fxNoise(q2 * 6.7, s2 &+ 0x9E3779B9)
    let rime = 0.24 * (1 - smoothstep(0.15, 0.6, dseg)) * smoothstep(0.35, 0.75, n) * dep * rfade
    return max(frost, max(rime, pane))
}

// brute-force crystal max over a wide index window, for the completeness check
func frostBrute(_ p: P, _ f: Ice, _ span: Int32) -> (F, F) {
    let c = f.unit, lu = f.len / c, rel = (p - f.tail) * (1 / c), perp = P(x: -f.dir.y, y: f.dir.x)
    let along = dot(rel, f.dir); let W = max(WIDTH, 0.75 / c), aa = 0.5 / c
    let sbase = fxHash(f.seed &+ ICE_SALT)
    let k0 = Int32((along / SPACING + 0.5).rounded(.down))
    var a3: F = 0, an: F = 0
    for dk in -span...span { let v = crystal(k0 + dk, rel, f, lu, perp, W, aa, sbase)
        an = max(an, v); if abs(dk) <= 1 { a3 = max(a3, v) } }
    return (a3, an)
}

// ---- the shards: hexagonal platelets shed at the melt, closed form
struct Shard { var centre: P; var radius: F; var glow: F; var cs: F; var sn: F }
let SHARDS_PER_UNIT: F = 1.5
var ROTATE = false
func shards(_ f: Ice) -> [Shard] {
    let c = f.unit, lu = f.len / c
    let n = Int(clampf(lu * SHARDS_PER_UNIT, 8, 96).rounded())
    var out: [Shard] = []
    for i in 0..<n {
        let h0 = fxHash(f.seed ^ (UInt32(i) &* 0x9E3779B9) ^ 0x5AAD_5AAD)
        let h1 = fxHash(h0), h2 = fxHash(h1), h3 = fxHash(h2), h4 = fxHash(h3), h5 = fxHash(h4)
        let along = fxUnit(h0)
        let born = HOLD + SPREAD * along + 0.3 * MELT * fxUnit(h5)
        let life = 0.20 + 0.15 * fxUnit(h1)
        let a = f.age - born
        if a < 0 || a > life { continue }
        let perp = P(x: -f.dir.y, y: f.dir.x)
        let p0 = f.tail + f.dir * (f.len * along) + perp * ((fxUnit(h2) - 0.5) * 0.7 * c)
        let v0 = P(x: (fxUnit(h3) - 0.5) * 1.2 * c, y: -(0.2 + 0.6 * fxUnit(h4)) * c)
        let K: F = 3, G: F = 6
        let s = (1 - exp(-K * a)) / K
        let drop = G * c / K * (a - s)
        let phase = F.pi * 2 * fxUnit(h1 ^ 0x5bd1e995)
        let heat = 1 - a / life
        let theta: F = ROTATE ? F.pi / 3 * fxUnit(h5) + (fxUnit(h4) - 0.5) * 8 * a : 0
        out.append(Shard(centre: P(x: p0.x + v0.x * s + 0.10 * c * sin(9 * a + phase), y: p0.y + v0.y * s + drop),
                         radius: c * (0.12 + 0.10 * fxUnit(h2)) * (0.6 + 0.4 * heat),
                         glow: 0.9 * heat, cs: cos(theta), sn: sin(theta)))
    }
    return out
}
func hexSd(_ p: P, _ r: F) -> F { // iq's sdHexagon (MIT)
    var q = P(x: abs(p.x), y: abs(p.y)); let k = P(x: -0.866025404, y: 0.5)
    q = q - k * (2 * min(dot(k, q), 0))
    q = q - P(x: clampf(q.x, -0.577350269 * r, 0.577350269 * r), y: r)
    return len(q) * (q.y < 0 ? -1 : 1)
}
func shardAlpha(_ uv: P) -> F {
    let d = hexSd(uv, 0.70)
    return (1 - smoothstep(-0.08, 0.12, d)) * (0.55 + 0.45 * smoothstep(-0.35, -0.02, d))
}

// ---- render
func render(_ name: String, _ W: Int, _ H: Int, _ ground: (F, F, F), _ cur: (F, F, F), _ f: Ice, _ withShards: Bool) {
    var px = [UInt8](repeating: 255, count: W * H * 4)
    let sh = withShards ? shards(f) : []
    for y in 0..<H { for x in 0..<W {
        let p = P(x: F(x) + 0.5, y: F(y) + 0.5)
        var a = iceAlpha(p, f)
        for s in sh {
            let side = max((2 * s.radius).rounded(.up), 2)
            let x0 = (s.centre.x - side * 0.5).rounded(.down), y0 = (s.centre.y - side * 0.5).rounded(.down)
            if p.x < x0 || p.x >= x0 + side || p.y < y0 || p.y >= y0 + side { continue }
            let e = P(x: (p.x - x0) / side * 2 - 1, y: (p.y - y0) / side * 2 - 1)
            let uv = P(x: s.cs * e.x - s.sn * e.y, y: s.sn * e.x + s.cs * e.y)
            let sa = s.glow * shardAlpha(uv)
            a = a + sa * (1 - a) // premultiplied OVER, one colour
        }
        let inCursor = p.x >= f.hole.l && p.x < f.hole.r && p.y >= f.hole.t && p.y < f.hole.b
        if inCursor { a = 1 }
        let i = (y * W + x) * 4
        px[i] = UInt8(clampf((cur.0 * a + ground.0 * (1 - a)) * 255, 0, 255).rounded())
        px[i + 1] = UInt8(clampf((cur.1 * a + ground.1 * (1 - a)) * 255, 0, 255).rounded())
        px[i + 2] = UInt8(clampf((cur.2 * a + ground.2 * (1 - a)) * 255, 0, 255).rounded())
    } }
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: &px, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W * 4, space: cs,
                        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let img = ctx.makeImage()!
    let url = URL(fileURLWithPath: name) as CFURL
    let dst = CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dst, img, nil); CGImageDestinationFinalize(dst)
}

func cell(_ col: F, _ row: F) -> Rect { Rect(l: col * 17, t: row * 34, r: (col + 1) * 17, b: (row + 1) * 34) }
func ice(_ from: Rect, _ to: Rect, _ age: F, _ seed: UInt32) -> Ice {
    let seg = Segment(from.corners, to)
    let tail = from.center, head = to.center
    let d = head - tail; let l = max(len(d), 1)
    return Ice(tail: tail, dir: d * (1 / l), len: l, unit: 34, age: age, seed: seed, quad: seg.at(age), hole: to)
}

let args = CommandLine.arguments
let out = args.count > 1 ? args[1] : "."
// completeness and reach checks
var maxDiff: F = 0; var worstReach: F = 0; var samples = 0; var nonzero = 0
var rng: UInt32 = 12345
func rnd() -> F { rng = fxHash(rng); return fxUnit(rng) }
for trial in 0..<60 {
    let fc = cell((rnd() * 100).rounded(.down), (rnd() * 12).rounded(.down))
    let tc = cell((rnd() * 100).rounded(.down), (rnd() * 12).rounded(.down))
    if abs(fc.l - tc.l) < 3 * 17 && abs(fc.t - tc.t) < 3 * 34 { continue }
    for age in [F(0.02), 0.05, 0.1, 0.2, 0.3, 0.45, 0.6, 0.7] {
        let f = ice(fc, tc, age, UInt32(trial) &* 7919)
        for _ in 0..<4000 {
            let p = P(x: rnd() * 1800, y: rnd() * 440)
            let (a3, an) = frostBrute(p, f, 5)
            maxDiff = max(maxDiff, abs(a3 - an)); samples += 1
            let a = iceAlpha(p, f)
            if a > 0 { nonzero += 1
                let rel = (p - f.tail) * (1 / f.unit); let al = dot(rel, f.dir)
                let dseg = len(rel - f.dir * clampf(al, 0, f.len / f.unit))
                let dq = quadDistance(p, f.quad) / f.unit
                worstReach = max(worstReach, min(dseg, dq)) }
        }
    }
}
var lastDeath: F = 0
for sd in 0..<200 { let f = ice(cell(3, 3), cell(90, 9), 0, UInt32(sd) &* 2654435761)
  var t: F = 0; while t < 1.2 { if !shards(Ice(tail: f.tail, dir: f.dir, len: f.len, unit: f.unit, age: t, seed: f.seed, quad: f.quad, hole: f.hole)).isEmpty { lastDeath = max(lastDeath, t) }; t += 0.002 } }
print("latest live shard over 200 seeds:", lastDeath, "bound", HOLD + SPREAD + 0.3 * MELT + 0.35, "; crystal bound", HOLD + SPREAD + 0.03 + MELT)

// ---- bands: one record per cell row, the x-interval the hull of the capsule's and the quad's
struct Rec { var x0, x1, y0, y1: F }
func interval(_ q: [P], _ lo: F, _ hi: F) -> (F, F)? {
    var xmin = F.infinity, xmax = -F.infinity
    for i in 0..<4 { let a = q[i], b = q[(i + 1) % 4]; var t0: F = 0, t1: F = 1; let dy = b.y - a.y
        if abs(dy) < 1e-6 { if a.y < lo || a.y > hi { continue } } else {
            let ta = (lo - a.y) / dy, tb = (hi - a.y) / dy; t0 = max(t0, min(ta, tb)); t1 = min(t1, max(ta, tb)); if t0 > t1 { continue } }
        for t in [t0, t1] { let x = a.x + (b.x - a.x) * t; xmin = min(xmin, x); xmax = max(xmax, x) } }
    return xmin <= xmax ? (xmin, xmax) : nil
}
func bands(_ f: Ice, _ fw: F, _ fh: F) -> [Rec] {
    let unit = f.unit, r = REACH * unit
    let head = f.tail + f.dir * f.len
    let cap = [f.tail, head, head, f.tail]
    let ys = (cap + f.quad).map { $0.y }
    let y0 = max(ys.min()! - r, 0), y1 = min(ys.max()! + r, fh)
    var band = (y0 / unit).rounded(.down) * unit; var out: [Rec] = []
    while band < y1 { let lo = band - r, hi = band + unit + r
        let ia = interval(cap, lo, hi), ib = interval(f.quad, lo, hi)
        if ia != nil || ib != nil {
            let xmin = min(ia?.0 ?? F.infinity, ib?.0 ?? F.infinity), xmax = max(ia?.1 ?? -F.infinity, ib?.1 ?? -F.infinity)
            let x0 = max(xmin - r, 0).rounded(.down), x1 = min(xmax + r, fw).rounded(.up)
            let top = max(band, 0).rounded(.down), bottom = min(band + unit, fh).rounded(.up)
            if x1 > x0 && bottom > top { out.append(Rec(x0: x0, x1: x1, y0: top, y1: bottom)) } }
        band += unit }
    return out
}
func bandCheck(_ f: Ice, _ fw: Int, _ fh: Int) -> (missed: Int, doubled: Int, lit: Int) {
    let recs = bands(f, F(fw), F(fh)); var missed = 0, doubled = 0, lit = 0
    for y in 0..<fh { for x in 0..<fw { let p = P(x: F(x) + 0.5, y: F(y) + 0.5)
        let n = recs.filter { p.x >= $0.x0 && p.x < $0.x1 && p.y >= $0.y0 && p.y < $0.y1 }.count
        if n > 1 { doubled += 1 }
        if iceAlpha(p, f) > 0 { lit += 1; if n == 0 { missed += 1 } } } }
    return (missed, doubled, lit)
}
func withUnit(_ f: Ice, _ u: F) -> Ice { var g = f; g.unit = u; return g }
// the retarget: the old ignition's path stays, the quad heads from near the old head to a new place
do {
    let from = cell(6, 3), to = cell(46, 3), again = cell(20, 11)
    var tot = (0, 0, 0)
    for age in [F(0.02), 0.06, 0.12, 0.2, 0.3] {
        var f = ice(from, to, age + 0.2, 77)
        f.quad = Segment(to.corners, again).at(age); f.hole = again
        let r = bandCheck(f, 1100, 440); tot = (tot.0 + r.0, tot.1 + r.1, tot.2 + r.2)
        for _ in 0..<20000 { let p = P(x: rnd() * 1100, y: rnd() * 440); if iceAlpha(p, f) > 0 {
            let rel = (p - f.tail) * (1 / f.unit); let al = dot(rel, f.dir)
            worstReach = max(worstReach, min(len(rel - f.dir * clampf(al, 0, f.len / f.unit)), quadDistance(p, f.quad) / f.unit)) } }
    }
    print("retarget bands: missed", tot.0, "doubled", tot.1, "of", tot.2, "lit pixels")
    var tot2 = (0, 0, 0)
    for age in [F(0.05), 0.2] { let r = bandCheck(ice(cell(4, 7), cell(60, 1), age, 9), 1100, 340); tot2 = (tot2.0 + r.0, tot2.1 + r.1, tot2.2 + r.2) }
    print("diagonal bands, unit 34: missed", tot2.0, "doubled", tot2.1, "of", tot2.2)
    let r3 = bandCheck(withUnit(ice(cell(4, 7), cell(60, 1), 0.2, 9), 33.5), 1100, 340)
    print("diagonal bands, unit 33.5 (fractional): missed", r3.0, "doubled", r3.1, "of", r3.2)
}
// the fallback that keeps rule 4: how long kitty's quad covers a point of the path
do {
    let from = cell(6, 4), to = cell(36, 4); let seg = Segment(from.corners, to)
    for u in [F(0.1), 0.25, 0.5, 0.75, 0.9] {
        let p = from.center + (to.center - from.center) * u
        var first: F = -1, last: F = -1; var t: F = 0
        while t < 0.6 { if quadDistance(p, seg.at(t)) < 0 { if first < 0 { first = t }; last = t }; t += 0.0005 }
        print("fallback: path point u =", u, "is inside the quad from", first, "to", last, "s")
    }
    print("segment lasts", seg.tau)
}
print("completeness: max |3-neighbour - brute(+-5)| =", maxDiff, "over", samples, "samples;", nonzero, "nonzero; worst min(dseg,dq) with a>0 =", worstReach)
// renders
let dark = (F(0x10) / 255, F(0x10) / 255, F(0x10) / 255), light = (F(0xE6) / 255, F(0xE6) / 255, F(0xE6) / 255)
let lg = (F(0xEF) / 255, F(0xEF) / 255, F(0xEF) / 255), dc = (F(0x27) / 255, F(0x27) / 255, F(0x27) / 255)
for (tag, from, to) in [("hop", cell(6, 4), cell(46, 4)), ("diag", cell(4, 7), cell(60, 1))] {
    for age in [F(0.03), 0.08, 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.8] {
        let f = ice(from, to, age, 42)
        let ms = Int((age * 1000).rounded())
        render("\(out)/\(tag)-dark-\(ms)ms.png", 1100, 340, dark, light, f, true)
        render("\(out)/\(tag)-light-\(ms)ms.png", 1100, 340, lg, dc, f, true)
    }
}
print("rendered")
