// short-circuit.criteria.js with an unrelated criterion that reaches a later
// turn. The first criterion's answer must not move.
criterion("two lands by turn 2", (t) =>
  t(1).count('t:land') >= 1 && t(2).count('t:land') >= 2,
  { atLeast: 0.30 });

criterion("any ramp by turn 5", (t) =>
  t(5).count('cat:"Ramp"') >= 1);
