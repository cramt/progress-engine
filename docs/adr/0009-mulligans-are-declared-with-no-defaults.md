# Mulligans are declared, with no defaults

`[mulligan]` has a `keep` rule, a `bottom` priority and a `down_to` floor, and none of the three has a default, because the keep rule is the pilot's and not the tool's ([#7](https://github.com/cramt/progress-engine/issues/7)). The model stays exact: under the London mulligan every depth is a fresh deal, so the answer is a sum of one enumeration per depth. Turn 0 is the hand that was kept. Every criterion is judged under the run's strategy, with the keep-your-seven number printed beside it ([#64](https://github.com/cramt/progress-engine/issues/64)). `optimise` weights live on `[mulligan]`, not on each criterion, and a criterion the exact engine can only estimate is refused from an objective ([#63](https://github.com/cramt/progress-engine/issues/63)).

## Considered Options

- **A default keep rule.** Rejected.
- **A Pareto frontier instead of file-declared weights.** Rejected.
- **Optimising over sampled conditionals.** Rejected.
