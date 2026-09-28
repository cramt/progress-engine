# gpu-spike

Issue [#65](https://github.com/cramt/progress-engine/issues/65)'s spike. It ports
the count-only walk to one CubeCL kernel and measures it against the exact engine.
This is not a product: nothing in `gauntlet` calls it, and CI does not build it.

## Run it

```
nix develop .#gpu-spike
cd crates/ichormoon-gauntlet/gpu-spike
cargo build --release
./target/release/gauntlet-gpu-spike ../../../decks/loam.deck.toml \
  ../../../decks/loam-access.criteria.toml --index ../../../decks/index.jsonl
```

The command prepares a criteria file exactly as `gauntlet test` does. Each class
that is a count-only walk is then answered three ways: by the engine, by the
kernel on CUDA, and by the kernel on CubeCL's CPU runtime. The answers print side
by side to the engine's six places.

Options:

- `--backend cuda|cpu|all` picks the backends.
- `--f32` accumulates in f32.
- `--no-engine` skips the engine.
- `--prefixes N` sets how many prefixes the host deals before the kernel takes
  over. `--prefixes 1` makes the kernel walk every checkpoint after the first.

## What is ported

| Part | How |
|---|---|
| The walk | `chip_stats::descend` becomes an odometer. The first composition of each checkpoint fills the last groups first, and each step is the lexicographic successor, which is the order the recursive walk uses. Sizes are fixed and there is no recursion. |
| The split | The host deals the first checkpoints with the engine's own `for_each_checkpoint_path`, and each thread walks everything below one prefix. The host sums each thread's totals in prefix order, so launch geometry cannot reach a digit. |
| The probability | Products of exact `C(n, k)` from a table, where the engine goes through log-gamma. |
| The criteria | The DNF that `gauntlet-toml` compiles, lowered to flat clause arrays. The hand, the library (the query's total less the hand), and the battlefield under the undeclared land drop (`Board::played_by`'s recurrence) are counted. The graveyard and casts are always 0 in a count-only walk. |

A class is refused when it has an effect, a casting line, a declared land drop, a
mulligan, a chosen strategy, a discard priority, a priced cost, or more than 32
groups.

Three seams in the product crates exist only for this, and each is behind a
`gpu-spike` feature that only this workspace turns on:

- `Criteria::counts_of` in `gauntlet-toml`.
- `gauntlet_cli::spike::prepare`, which builds each class's narrowed grouping and
  schedule the way `answer` does.
- `MAX_PATHS` lifted in `gauntlet-criteria`, so the engine can be timed past its
  ceiling.

## Findings

Measured on luna: a GTX 1660 (Turing, 6 GB, f64 at 1/32 of f32), driver 595 with
CUDA 13.2 and NVRTC 12.9, and an 8-thread CPU. CubeCL is 0.10.0.

### Agreement

| Criteria file | Seat | Paths | Engine vs CUDA vs CubeCL CPU |
|---|---|---|---|
| `decks/loam-access`, all 4 count-only classes | play, draw | 2 – 972 | same to 6 places, \|diff\| ≤ 3.5e-14 (with `--prefixes 1`) |
| `bench/lantern-curve` | play | 219,031,311 | same to 6 places, \|diff\| ≤ 1.5e-14 |
| `bench/lantern-curve-unsplit` | play | 219,031,311 | same to 6 places, \|diff\| ≤ 1.5e-14 |
| `bench/lantern-curve-drops`, land-drop class | play | 461,249,362 | same to 6 places, \|diff\| ≤ 5.2e-15 (engine vs CUDA) |

Mass sits within 1e-13 of 1 everywhere in f64.

### Throughput

All times are for the 219M-path class of `bench/lantern-curve` (9 groups, gaps
`[7,0,1,1,1,1,1]`):

| Backend | Warm | Cold (includes JIT) | Paths/s |
|---|---|---|---|
| CUDA, GTX 1660, f64 | 1.12 s | 3.2 s | 196 M |
| CUDA, GTX 1660, f32 | 1.12 s | 3.9 s | 196 M |
| CubeCL CPU runtime, f64 | 60.1 s | 60.7 s | 3.6 M |
| Engine, 8 threads | 172.7 s | — | 1.3 M |
| Engine, unsplit shape (`[0,7,1,…]`) | 580.9 s | — | 0.4 M |

1. **f64 is free here, and f32 is not exact.** f32 runs at the same speed
   (1.117 s against 1.120 s), so the kernel is bound by integers and memory, not
   by float throughput, even on a card with f64 at 1/32 rate. f32's mass is off
   by 4.9e-7, which fails the engine's 1e-9 `MASS_TOLERANCE`. The f64 concern in
   #65 does not bind a count-only walk. That leaves CUDA, ROCm and Vulkan with
   f64, as #65 said.
2. **The engine is far slower on this shape than #65's Lantern number
   suggests.** #65 quotes 660M compositions in 14 s on 4 cores, about 47M/s.
   Here the engine does 1.3M paths/s on 8 threads, on a walk that has no mana and
   no effects. This is not investigated yet, so the 150× GPU lead should not be
   read as the GPU's lead on a real run.
3. **A count-only class that does not read turn 0 runs on one thread.**
   Cumulative narrowing gives it gaps `[0, 7, 1, …]`, and `run_answering` splits
   at the opener only when `gaps[0] > 0`. It falls into the direct walk, which is
   documented as "a single multivariate hypergeometric, which is instant". The
   unsplit copy of the bench took 581 s, against 173 s for the same walk with a
   turn-0 clause. This is an engine bug independent of the GPU.
4. **The committed decks' count-only classes are too small for a GPU to matter.**
   Narrowing collapses them to 2–972 paths, and there the GPU's ~0.3 ms launch
   plus 0.1–0.9 s cold compile is all it costs. The count-only walk this spike
   ports is not where a real run spends its time. The wide classes are the ones
   that price mana or cast.
5. **CubeCL's CPU runtime cannot replace `Board`.** It keeps about 2 of 8 cores
   busy at 3.6M paths/s, and its build downloads a prebuilt LLVM/MLIR 20
   (`tracel-llvm-bundler`) that a Nix sandbox build could not do. It also needs a
   libstdc++ at run time.
6. **Land drops, the first feature added.** The undeclared land drop is a `min`
   recurrence, so it adds no divergence. With it, a 10-group, 461M-path class
   runs at 140M paths/s, against 196–211M without it on 9 groups. This does not
   measure divergence; a declared `[land_drop]` priority would.

### Pitfall

A `let mut` started from a comptime value stays comptime in CubeCL: `d += 1`
inside a runtime loop runs once, at expansion. Such a kernel compiles and runs
and returns the wrong mass. Lift the value with `.runtime()`. `--prefixes 1` is
what caught it: at the default split, the host dealt every checkpoint of the
committed classes and the kernel's own walk never ran.

## What this says about the decision

The part of `Board` that is cheap to port is also the part that costs nothing
today. Before the rest is rewritten into the kernel subset, the next spike should:

- find why the engine walks this shape at 1.3M/s, and fix the unsplit
  single-thread path;
- port the declared land drop, which is the first real per-path branch, and
  measure divergence on a class that prices mana.
