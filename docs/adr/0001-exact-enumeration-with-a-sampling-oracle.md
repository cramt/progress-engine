# Exact enumeration, with a sampler kept only as an oracle

Every number Ichormoon Gauntlet reports comes from enumerating compositions. `gauntlet-sim` is a second implementation that exists to disagree with it, and the two are asserted to agree at three levels. The tooling this replaced trusted two PRNG prototypes, and both produced confidently wrong numbers that nothing caught. So a feature that cannot stay exact needs a very good reason, and teaching the exact engine something means teaching the sampler the same thing.

## Considered Options

- **Sampling only**, as the predecessor did. Rejected, because nothing checks a sampler.
- **A card-level `--simulate` API** ([#11](https://github.com/cramt/progress-engine/issues/11)). Rejected, because it stops criteria being pure functions of counts.

See [VISION.md: Exact, with a second implementation as the oracle](../../VISION.md#exact-with-a-second-implementation-as-the-oracle).
