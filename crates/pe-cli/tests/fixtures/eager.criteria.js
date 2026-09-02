// The same question as short-circuit.criteria.js, with both operands evaluated
// whatever the first one says.
criterion("two lands by turn 2", (t) => {
  const a = t(1).count('t:land') >= 1;
  const b = t(2).count('t:land') >= 2;
  return a && b;
}, { atLeast: 0.30 });
