// A distribution wide enough that the terminal cannot show all of it. Lands seen
// by turn 20 runs from 0 to 26, and the report prints the twelve buckets holding
// the most mass and says how much it left out.
expect("lands by turn 20", (t) => t(20).count('t:land'));
