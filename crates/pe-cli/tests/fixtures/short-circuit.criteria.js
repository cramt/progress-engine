// The deepest turn this file names sits behind a `&&` that is false while every
// count is zero, which is what an opening probe sees. Both spellings of the same
// question must agree; they once differed by the whole answer.
criterion("two lands by turn 2", (t) =>
  t(1).count('t:land') >= 1 && t(2).count('t:land') >= 2,
  { atLeast: 0.30 });
