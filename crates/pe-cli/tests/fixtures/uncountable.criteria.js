// An expectation computing a rate rather than a count. There is no bucket for
// 0.36, and rounding it to fit one would answer a different question in silence.
expect("lands per card", (t) => t(0).count('t:land') / 7);
