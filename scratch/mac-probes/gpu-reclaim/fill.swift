// Fills the GPU working set with shared buffers and keeps compute in flight until killed.
// usage: fill <GiB> ; prints "ready <bytes>" once allocated.
import Metal
import Foundation
setvbuf(stdout, nil, _IOLBF, 0)
let gib = Double(CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "8") ?? 8
let dev = MTLCreateSystemDefaultDevice()!
let q = dev.makeCommandQueue()!
let src = "kernel void k(device float *a [[buffer(0)]], uint i [[thread_position_in_grid]]) { float x=a[i]; for(int n=0;n<256;n++) x=x*1.0001f+0.5f; a[i]=x; }"
let lib = try! dev.makeLibrary(source: src, options: nil)
let pso = try! dev.makeComputePipelineState(function: lib.makeFunction(name: "k")!)
let chunk = 256 << 20
var bufs: [MTLBuffer] = []
while Double(bufs.count * chunk) < gib * 1073741824.0 {
    guard let b = dev.makeBuffer(length: chunk, options: .storageModeShared) else { break }
    memset(b.contents(), 1, chunk)  // touch so pages are resident
    bufs.append(b)
}
print("ready \(bufs.count * chunk) cap \(dev.recommendedMaxWorkingSetSize) cur \(dev.currentAllocatedSize)")
signal(SIGTERM) { _ in print("sigterm"); exit(0) }
while true {
    let cb = q.makeCommandBuffer()!
    for b in bufs.prefix(8) {
        let e = cb.makeComputeCommandEncoder()!
        e.setComputePipelineState(pso); e.setBuffer(b, offset: 0, index: 0)
        e.dispatchThreads(MTLSize(width: chunk / 4, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
        e.endEncoding()
    }
    cb.commit(); cb.waitUntilCompleted()
}
