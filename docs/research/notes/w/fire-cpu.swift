// CPU cost of the same flame field, scalar Swift -O, over the banded cover the GPU path draws.
import Foundation

typealias V2 = SIMD2<Float>
let cellW: Float = 17, cellH: Float = 34
let FLAME_H: Float = 1.6, RISE: Float = 2.2, DRIFT: Float = 0.6, OCT = 4
let BB: [SIMD3<Float>] = [
    [1.000, 0.000, 0.000], [1.000, 0.000, 0.000], [1.000, 0.097, 0.000], [1.000, 0.206, 0.000], [1.000, 0.303, 0.000],
    [1.000, 0.395, 0.000], [1.000, 0.482, 0.000], [1.000, 0.563, 0.109], [1.000, 0.637, 0.263], [1.000, 0.705, 0.391],
    [1.000, 0.766, 0.510], [1.000, 0.820, 0.623], [1.000, 0.868, 0.729], [1.000, 0.910, 0.827], [1.000, 0.945, 0.916],
    [1.000, 0.976, 0.997],
].map { SIMD3<Float>($0[0], $0[1], $0[2]) }

@inline(__always) func pcg(_ v: UInt32) -> UInt32 {
    let s = v &* 747796405 &+ 2891336453
    let w = ((s >> ((s >> 28) &+ 4)) ^ s) &* 277803737
    return (w >> 22) ^ w
}
@inline(__always) func u01(_ h: UInt32) -> Float { Float(bitPattern: (h >> 9) | 0x3f80_0000) - 1 }
@inline(__always) func lattice(_ x: Int32, _ y: Int32, _ seed: UInt32) -> Float { u01(pcg(UInt32(bitPattern: x) &+ pcg(UInt32(bitPattern: y) &+ seed))) }
@inline(__always) func mix(_ a: Float, _ b: Float, _ t: Float) -> Float { a + (b - a) * t }
@inline(__always) func vnoise(_ p: V2, _ seed: UInt32) -> Float {
    let fx = p.x.rounded(.down), fy = p.y.rounded(.down)
    let ix = Int32(fx), iy = Int32(fy)
    let x = p.x - fx, y = p.y - fy
    let wx = x * x * x * (x * (x * 6 - 15) + 10), wy = y * y * y * (y * (y * 6 - 15) + 10)
    let a = lattice(ix, iy, seed), b = lattice(ix + 1, iy, seed), c = lattice(ix, iy + 1, seed), d = lattice(ix + 1, iy + 1, seed)
    return mix(mix(a, b, wx), mix(c, d, wx), wy)
}
@inline(__always) func fbm(_ p0: V2, _ seed: UInt32) -> Float {
    var t: Float = 0, a: Float = 0.5, p = p0
    for o in 0 ..< OCT {
        t += a * vnoise(p, seed &+ UInt32(o) &* 0x9E37_79B9)
        p = V2(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y) * 2.02
        a *= 0.5
    }
    return t
}
@inline(__always) func seg(_ p: V2, _ a: V2, _ b: V2) -> Float {
    let pa = p - a, ba = b - a
    let h = min(max((pa * ba).sum() / max((ba * ba).sum(), 1e-6), 0), 1)
    let d = pa - ba * h; return (d * d).sum().squareRoot()
}
@inline(__always) func xing(_ p: V2, _ a: V2, _ b: V2) -> Bool {
    var dy = b.y - a.y; if abs(dy) < 1e-6 { dy = 1e-6 }
    return ((a.y > p.y) != (b.y > p.y)) && (p.x < (b.x - a.x) * (p.y - a.y) / dy + a.x)
}
@inline(__always) func sdQuad(_ p: V2, _ q: [V2]) -> Float {
    let d = min(min(seg(p, q[0], q[1]), seg(p, q[1], q[2])), min(seg(p, q[2], q[3]), seg(p, q[3], q[0])))
    let inside = xing(p, q[0], q[1]) != xing(p, q[1], q[2]) != xing(p, q[2], q[3]) != xing(p, q[3], q[0])
    return inside ? -d : d
}
@inline(__always) func smooth(_ a: Float, _ b: Float, _ x: Float) -> Float { let t = min(max((x - a) / (b - a), 0), 1); return t * t * (3 - 2 * t) }

struct F { var q: [V2]; var hole: SIMD4<Float>; var dir: V2; var tail: V2; var len: Float; var age: Float; var life: Float; var seed: UInt32 }
@inline(__always) func fire(_ p: V2, _ f: F, simple: Bool) -> SIMD4<Float> {
    let c = cellH
    if p.x >= f.hole.x && p.x < f.hole.z && p.y >= f.hole.y && p.y < f.hole.w { return .zero }
    let k = f.age / f.life; if k >= 1 { return .zero }
    let d0 = sdQuad(p, f.q) / c
    if d0 > FLAME_H + 0.4 { return .zero }
    let u = min(max(((p - f.tail) * f.dir).sum() / max(f.len, 1), 0), 1)
    let burn = smooth(k - 0.15, k + 0.35, u) * (1 - k * k)
    let cool = min(max(max(d0, 0) / FLAME_H, 0), 1)
    var heat: Float
    if simple {                                            // the no-noise fallback: a hot band with a soft halo
        heat = (1 - smooth(-0.25, 0.30, d0)) * (1 - 0.85 * cool) * 0.8 * burn
    } else {
        let q = p / c
        let s = V2(q.x * 2.6, (q.y + f.age * RISE) * 0.7) - f.dir * (f.age * DRIFT)
        let n = fbm(s, f.seed)
        let lift = FLAME_H * c * smooth(0.25, 0.85, n)
        let d = sdQuad(p + V2(0, lift), f.q) / c
        let body = max(1 - smooth(-0.10, 0.20, d), 0.8 * (1 - smooth(-0.25, 0.05, d0)))
        heat = body * (1 - 0.85 * cool) * (0.50 + 0.60 * n) * burn
    }
    let halo = 0.12 * exp(-2.5 * max(d0, 0)) * burn
    if heat <= 0.02 { let hc = BB[5] * halo; return SIMD4(hc.x, hc.y, hc.z, 0) }
    let h = min(heat, 1), x = h * 15; let i = min(Int(x), 14); let fr = x - Float(i)
    let col = BB[i] + (BB[i + 1] - BB[i]) * fr
    let a = smooth(0.05, 0.45, h), glow = smooth(0, 0.6, h)
    let rgb = col * glow + BB[5] * halo
    return SIMD4(rgb.x, rgb.y, rgb.z, a * 0.35)
}

// geometry: a static parallelogram between two cells plus its dilated cover, as the GPU harness draws
func rect(_ r: Int, _ c: Int) -> (V2, V2) { let x = Float(c) * cellW, y = Float(r) * cellH; return (V2(x, y), V2(x + cellW, y + cellH)) }
func setup(_ from: (Int, Int), _ to: (Int, Int)) -> (F, [(Int, Int, Int, Int)]) {
    let (pa, pb) = rect(from.0, from.1), (ta, tb) = rect(to.0, to.1)
    let q = [V2(tb.x, ta.y), V2(tb.x, tb.y), V2(pa.x, pb.y), V2(pa.x, pa.y)]
    let tail = (pa + pb) * 0.5, head = (ta + tb) * 0.5, dv = head - tail
    let len = max((dv * dv).sum().squareRoot(), 1)
    let f = F(q: q, hole: SIMD4(ta.x, ta.y, tb.x, tb.y), dir: dv / len, tail: tail, len: len, age: 0.1, life: 0.45, seed: 0x1234_5678)
    // cover: bands of one cell height, x-range of the dilated quad
    let R = (FLAME_H + 0.4) * cellH
    var out = [(Int, Int, Int, Int)]()
    let ys = q.map { $0.y }
    var yb = ((ys.min()! - R) / cellH).rounded(.down) * cellH
    while yb < ys.max()! + R {
        let lo = yb - R, hi = yb + cellH + R
        var xmin = Float.infinity, xmax = -Float.infinity
        for i in 0 ..< 4 {
            let a = q[i], b = q[(i + 1) % 4]; var t0: Float = 0, t1: Float = 1; let dy = b.y - a.y
            if abs(dy) < 1e-6 { if a.y < lo || a.y > hi { continue } } else {
                var ta = (lo - a.y) / dy, tb = (hi - a.y) / dy; if ta > tb { swap(&ta, &tb) }
                t0 = max(t0, ta); t1 = min(t1, tb); if t0 > t1 { continue }
            }
            for t in [t0, t1] { let x = a.x + (b.x - a.x) * t; xmin = min(xmin, x); xmax = max(xmax, x) }
        }
        if xmin <= xmax { out.append((Int(max(0, xmin - R)), Int(max(0, yb)), Int(xmax + R - max(0, xmin - R)), Int(cellH))) }
        yb += cellH
    }
    return (f, out)
}
var sink = SIMD4<Float>.zero
func run(_ name: String, _ from: (Int, Int), _ to: (Int, Int), step: Int, simple: Bool) {
    let (f, cover) = setup(from, to)
    let px = cover.reduce(0) { $0 + $1.2 * $1.3 } / (step * step)
    var times = [Double]()
    for rep in 0 ..< 9 {
        let t0 = DispatchTime.now().uptimeNanoseconds
        for (x0, y0, w, h) in cover {
            var y = y0; while y < y0 + h { var x = x0; while x < x0 + w {
                sink += fire(V2(Float(x) + 0.5, Float(y) + 0.5), f, simple: simple); x += step }; y += step }
        }
        let t1 = DispatchTime.now().uptimeNanoseconds
        if rep > 0 { times.append(Double(t1 - t0) / 1e3) }
    }
    times.sort()
    print(String(format: "%-28@ evaluated px %8d   us/frame median %8.1f [%.1f-%.1f]   ns/px %.1f", name as NSString, px, times[times.count / 2], times.first!, times.last!, times[times.count / 2] * 1e3 / Double(px)))
}
run("hop30 full-res noise", (20, 40), (20, 70), step: 1, simple: false)
run("hop30 half-res noise", (20, 40), (20, 70), step: 2, simple: false)
run("hop30 full-res no-noise", (20, 40), (20, 70), step: 1, simple: true)
run("diag full-res noise", (2, 2), (48, 197), step: 1, simple: false)
run("diag half-res noise", (2, 2), (48, 197), step: 2, simple: false)
run("diag full-res no-noise", (2, 2), (48, 197), step: 1, simple: true)
print("sink", sink.sum())
