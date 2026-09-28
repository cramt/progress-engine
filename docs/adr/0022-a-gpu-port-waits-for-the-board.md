# A GPU port waits until the board's branches are measured, and the walk keeps the shape one would need

[#65](https://github.com/cramt/progress-engine/issues/65)'s spike ([#126](https://github.com/cramt/progress-engine/pull/126), branch `gpu-spike-65`, kept unmerged) ported the count-only walk to one CubeCL kernel. On a GTX 1660 in f64, it agreed with the exact engine to 1.5e-14 on a 219M-path Lantern class and a 461M-path one. It walked the first in 1.12 s, where the engine took 172.7 s on 8 threads. **That speed-up is on the part of the walk that costs nothing today.** The committed decks' count-only classes narrow to 2–972 paths. The classes that are wide are the ones that price mana or cast, and those run on `Board`, which the spike did not port. So there is no GPU backend until the declared land drop, the first real per-path branch, has been ported and its divergence measured on a class that prices mana.

Until then, the walk keeps the shape that the kernel relied on, so that a port stays a translation and not a redesign:

- **The descent order is a contract.** `chip_stats` deals a checkpoint's compositions in lexicographic order of their counts, starting from the one that puts as many cards as it can in the last groups. A kernel can therefore step from one composition to the next without recursion, like an odometer, and visit paths in the same order the engine sums them.
- **A path's probability is a product over its checkpoints**, one multivariate hypergeometric each, and splits are summed on one thread in a fixed order (ADR-0013). Launch geometry reaches no digit for the same reason thread count does not.
- **A path's state is counts, bounded by groups × checkpoints.** New board state should fit in fixed-size arrays, with no allocation per path.
- **Prefer a recurrence over a branch where both are exact.** The undeclared land drop is `min(drawn, played + 1)` a turn. It has no branch, so it adds no divergence: with it, a 10-group class ran at 140M paths/s. A declared priority is a branch, and branches are what a port pays for.
- **f64 throughout.** An f32 walk ran no faster on a card with 1/32-rate f64, and its mass missed `MASS_TOLERANCE` by 4.9e-7. That rules out WebGPU and Metal, and leaves CUDA, ROCm and Vulkan.

The spike also found two things about the engine on its own:

- A class that reads no turn 0 is narrowed to a schedule whose first checkpoints are empty. The walk split only at checkpoint 0, so the class became one continuation on one thread: 581 s against 173 s for the same walk with a turn-0 clause. **Fixed:** the walk splits at the first checkpoint that deals anything, and the empty ones stay in the history, because a checkpoint's index is the turn it names.
- On the spike's count-only bench the engine walks 1.3M paths/s on 8 threads, against the ~47M/s that #65 quotes for Lantern. Nobody has explained this yet. Until it is profiled, the GPU's lead over the engine is not a number to quote.

## Considered Options

- **Merge the spike as a crate.** Rejected. CI can build neither backend: there is no GPU for CUDA, and CubeCL's CPU runtime downloads a prebuilt LLVM, which a Nix sandbox refuses. A crate that nothing builds rots without anyone noticing. Its seams in the product crates included a feature that lifts `MAX_PATHS`, and a feature that removes a limit is not additive.
- **CubeCL's CPU runtime in place of `Board`.** Rejected: 3.6M paths/s with about 2 of 8 cores busy, the same LLVM download, and a libstdc++ needed at run time.
- **wgpu.** Rejected while numbers need f64.

CubeCL has one pitfall to remember next time. A `let mut` that starts from a comptime value stays comptime, so `d += 1` in a runtime loop runs once, at expansion. The kernel compiles and returns the wrong mass. Lift the value with `.runtime()`, and test with the host dealing as little as possible, or the kernel's own walk never runs.

See [VISION.md: Exact, with a second implementation as the oracle](../../VISION.md#exact-with-a-second-implementation-as-the-oracle).
