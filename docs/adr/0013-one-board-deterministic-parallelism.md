# Both engines share one rules implementation, and parallelism moves no digit

The exact engine and the sampler walk the same `Board` and differ only in how they produce paths. That way agreement between them tests the enumeration, not two copies of the rules. Multi-core walks split at the opener and are summed on one thread in opener order, so `GAUNTLET_THREADS=1` and eight threads print identical bytes. The probability-mass check uses Kahan summation, because naive error would reach the size of the smallest path.

## Considered Options

- **fearless_simd.** Rejected on measurement: 7.4s became 9.5s.
- **A hand-written GPU kernel.** Deferred, because it would be the first second copy of the rules ([#65](https://github.com/cramt/progress-engine/issues/65)).
