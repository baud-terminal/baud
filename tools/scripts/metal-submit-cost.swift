// What one Metal command buffer costs the CPU to submit, and whether what it
// binds changes that. One frame in flight at a time, as baud runs; the
// process's CPU (user and system, `getrusage`) over N frames, arms
// interleaved and rotated, the first round dropped.
//
//   nothing     the pacing alone: no Metal at all
//   empty       a cleared render pass into an IOSurface-backed target
//   bound       the same, plus a draw binding what the cell path binds -- a
//               grid, a 65,536-slot style table, a glyph map, an overlay and
//               two 2048x2048 sheets
//   untracked   the same resources with hazard tracking off
//   residency   the untracked resources in a committed residency set
//   unretained  a command buffer that does not retain its references
//
// Build with the real Xcode toolchain (see tools/scripts/winid.swift for why):
//
//   DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//   /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
//     -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
//     -o metal-submit-cost tools/scripts/metal-submit-cost.swift
//
//   ./metal-submit-cost 600            # back to back
//   ./metal-submit-cost 120 16000      # one frame every 16 ms, as at 60 Hz
import Metal
import IOSurface
import Foundation

let dev = MTLCreateSystemDefaultDevice()!
let queue = dev.makeCommandQueue()!
let W = 3200, H = 2000
func surfaceTex(_ i: Int) -> MTLTexture {
    let props: [IOSurfacePropertyKey: Any] = [
        .width: W, .height: H, .bytesPerElement: 4, .pixelFormat: 0x42475241, // 'BGRA'
    ]
    let s = IOSurface(properties: props)!
    let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: W, height: H, mipmapped: false)
    d.usage = [.renderTarget, .shaderRead]
    d.storageMode = .shared
    return dev.makeTexture(descriptor: d, iosurface: unsafeBitCast(s, to: IOSurfaceRef.self), plane: 0)!
}
let targets = (0..<3).map(surfaceTex)
let src = """
#include <metal_stdlib>
using namespace metal;
struct O { float4 p [[position]]; };
vertex O v(uint i [[vertex_id]], const device uint *a [[buffer(0)]], const device uint *b [[buffer(1)]],
           const device uint *c [[buffer(3)]], const device uint *d [[buffer(4)]]) {
  O o; float x = (i & 1) ? 1.0 : -1.0; float y = (i & 2) ? 1.0 : -1.0;
  o.p = float4(x * 0.001 + float(a[0] + b[0] + c[0] + d[0]) * 0.0, y * 0.001, 0, 1); return o; }
fragment half4 f(O in [[stage_in]], texture2d<half> m [[texture(0)]], texture2d<half> k [[texture(1)]]) {
  constexpr sampler s; return m.sample(s, float2(0)) + k.sample(s, float2(0)); }
"""
let lib = try! dev.makeLibrary(source: src, options: nil)
let pd = MTLRenderPipelineDescriptor()
pd.vertexFunction = lib.makeFunction(name: "v"); pd.fragmentFunction = lib.makeFunction(name: "f")
pd.colorAttachments[0].pixelFormat = .bgra8Unorm
let pipe = try! dev.makeRenderPipelineState(descriptor: pd)

func resources(_ opts: MTLResourceOptions) -> ([MTLBuffer], [MTLTexture]) {
    let sizes = [400 * 150 * 8, 65536 * 32, 16384 * 24, 400 * 150]
    let bufs = sizes.map { dev.makeBuffer(length: $0, options: opts)! }
    func tex(_ pf: MTLPixelFormat) -> MTLTexture {
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: pf, width: 2048, height: 2048, mipmapped: false)
        d.usage = .shaderRead; d.resourceOptions = opts; return dev.makeTexture(descriptor: d)!
    }
    return (bufs, [tex(.r8Unorm), tex(.bgra8Unorm)])
}
let tracked = resources([.storageModeShared])
let untracked = resources([.storageModeShared, .hazardTrackingModeUntracked])

var residency: MTLResidencySet? = nil
if #available(macOS 15.0, *) {
    let rd = MTLResidencySetDescriptor(); rd.initialCapacity = 16
    let rs = try! dev.makeResidencySet(descriptor: rd)
    for b in untracked.0 { rs.addAllocation(b) }
    for t in untracked.1 { rs.addAllocation(t) }
    for t in targets { rs.addAllocation(t) }
    rs.commit(); rs.requestResidency()
    residency = rs
}

func cpu() -> Double {
    var u = rusage(); getrusage(RUSAGE_SELF, &u)
    return Double(u.ru_utime.tv_sec + u.ru_stime.tv_sec) + Double(u.ru_utime.tv_usec + u.ru_stime.tv_usec) / 1e6
}

enum Arm: String, CaseIterable { case nothing, empty, bound, untracked, residency, unretained }
func frame(_ arm: Arm, _ n: Int) {
    if arm == .nothing { return }
    let t = targets[n % 3]
    let pass = MTLRenderPassDescriptor()
    pass.colorAttachments[0].texture = t
    pass.colorAttachments[0].loadAction = .clear
    pass.colorAttachments[0].storeAction = .store
    let cmd: MTLCommandBuffer
    if arm == .unretained { cmd = queue.makeCommandBufferWithUnretainedReferences()! } else { cmd = queue.makeCommandBuffer()! }
    if arm == .residency, #available(macOS 15.0, *), let rs = residency { cmd.useResidencySet(rs) }
    let enc = cmd.makeRenderCommandEncoder(descriptor: pass)!
    if arm != .empty {
        let (bufs, texs) = (arm == .bound || arm == .unretained) ? tracked : untracked
        enc.setRenderPipelineState(pipe)
        enc.setVertexBuffer(bufs[0], offset: 0, index: 0)
        enc.setVertexBuffer(bufs[1], offset: 0, index: 1)
        enc.setVertexBuffer(bufs[2], offset: 0, index: 3)
        enc.setVertexBuffer(bufs[3], offset: 0, index: 4)
        enc.setFragmentTexture(texs[0], index: 0)
        enc.setFragmentTexture(texs[1], index: 1)
        enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
    }
    enc.endEncoding()
    let sem = DispatchSemaphore(value: 0)
    cmd.addCompletedHandler { _ in sem.signal() }
    cmd.commit()
    sem.wait()
}

let N = Int(CommandLine.arguments.dropFirst().first ?? "600")!
let gapUs = UInt32(CommandLine.arguments.dropFirst(2).first ?? "0")!
let rounds = 6
var res: [Arm: [Double]] = [:]
for r in 0..<rounds {
    let order = Arm.allCases.enumerated().map { Arm.allCases[($0.offset + r) % Arm.allCases.count] }
    for arm in order {
        for i in 0..<20 { frame(arm, i) }
        let c0 = cpu()
        for i in 0..<N { frame(arm, i); if gapUs > 0 { usleep(gapUs) } }
        let per = (cpu() - c0) / Double(N) * 1e6
        if r > 0 { res[arm, default: []].append(per) }
        print(String(format: "round %d %-10@ %7.1f us CPU a frame", r, arm.rawValue as NSString, per))
    }
}
for arm in Arm.allCases {
    let v = res[arm]!.sorted()
    print(String(format: "%-10@ median %6.1f [%6.1f-%6.1f] us", arm.rawValue as NSString, v[v.count / 2], v.first!, v.last!))
}
