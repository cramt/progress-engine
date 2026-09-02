// Asserts something the deck cannot do, to prove a missed threshold fails.
criterion("all seven opening cards are one-mana ramp", (t) =>
  t(0).count('cat:"Ramp - One Mana"') >= 7,
  { atLeast: 0.50 });
