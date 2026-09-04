// A criterion that forgot its comparison. JavaScript would coerce the count to
// a bool and answer "at least one land" under a name that promises a number.
criterion("lands in opener", (t) => t(0).count('t:land'));
