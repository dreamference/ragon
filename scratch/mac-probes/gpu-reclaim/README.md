# GPU memory reclaim after a kill (Mac node spec §4.2, second probe)

`fill.swift` allocates N GiB of shared Metal buffers, touches them and keeps compute in flight.
`run.sh <SIGKILL|SIGTERM> <GiB>` starts it, samples `vm_stat`, `memory_pressure` and the
IOAccelerator "In use system memory" once a second (8 s before, 40 s after the kill). Raw output: `results/`.
Run on the Mac Studio (macOS 27.2, 128 GB, recommended working set 115.4 GB), nobody else loading it.

| Run | Allocated | Free pages before | Free pages 1 s after kill | Back to baseline |
| --- | --- | --- | --- | --- |
| SIGKILL, work in flight | 16 GiB | 5.53 M | 6.59 M | first sample after (<= 1 s) |
| SIGTERM (handler exits), work in flight | 16 GiB | 5.53 M | 6.59 M | first sample after (<= 1 s) |
| SIGKILL, work in flight | 95 GiB (102 GB resident) | 0.35 M (5.7 GB free) | 6.59 M | first sample after (<= 1 s) |

Findings: all memory returns within the 1 s sampling interval, for both signals; no leaked wired
or compressed pages, swapouts counter unchanged (609392) in every run. A shell on the box answered in
10 ms at 5.7 GB free, so the desktop stayed responsive.

Limits: the 95 GiB run did not produce real pressure (no swap, compressor flat, `memory_pressure`
still said 94 % free, because it counts reclaimable pages), so G15's trip thresholds still need a
run with other apps resident. The IOAccelerator counter does not track the allocation (it stayed near
2.3 GB); `vm_stat` free pages are the usable signal. Responsiveness was a remote shell, not the GUI.
Sub-second timing needs a finer sampler.
