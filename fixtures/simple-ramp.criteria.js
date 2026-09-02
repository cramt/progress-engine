// What this deck has to do to function.
//
// t(n) is your position on turn n; t(0) is the opening hand. On the play turn 1
// draws nothing, so t(0) and t(1) see the same seven cards.

criterion("keepable opener (2-5 lands)", (t) =>
  t(0).count('t:land') >= 2 && t(0).count('t:land') <= 5,
  { atLeast: 0.70 });

criterion("turn-1 accelerant", (t) =>
  t(1).count('t:land') >= 1 &&
  t(1).count('cat:"Ramp - One Mana"') >= 1,
  { atLeast: 0.35 });

// The bar for this deck getting off the ground: a land and a one-mana
// accelerant on turn one, then a second land, so the three-mana commander
// lands on turn two instead of turn three.
criterion("commander on turn 2", (t) =>
  t(1).count('t:land') >= 1 &&
  t(1).count('cat:"Ramp - One Mana"') >= 1 &&
  t(2).count('t:land') >= 2,
  { atLeast: 0.30 });

// Informational: no threshold, so it reports a number and cannot fail.
criterion("any ramp by turn 3", (t) =>
  t(3).count('cat:"Ramp - One Mana"') + t(3).count('cat:"Ramp"') >= 1);
