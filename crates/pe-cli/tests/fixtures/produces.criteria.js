// Queries that only work against an index this tool built itself.
//
// `produces:` is the fix for the bug the README opens with: it asks what a card
// makes rather than what its text mentions. `c:` is the card's own colour,
// which is not its colour identity.
criterion("green source in opener", (t) => t(0).count('produces:g') >= 1, { atLeast: 0.9 });
criterion("turn-1 green creature", (t) => t(0).count('c:g t:creature mv=1') >= 1);
expect("green sources in opener", (t) => t(0).count('produces:g'));
