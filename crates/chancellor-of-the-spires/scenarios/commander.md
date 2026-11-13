# Commander

### 1. Doctor's companion pairs only with a Time Lord Doctor, not any Doctor

**Cards:** [Clara Oswald], [Doctor Strange, Surgeon], [The Second Doctor]
**Board:** Deck construction: P1 designates two commanders.
**Does:** (a) Clara Oswald and Doctor Strange, Surgeon. (b) Clara Oswald
and The Second Doctor.
**Then:** (a) is refused: Doctor Strange is a Human Doctor Hero, not a
Time Lord Doctor. (b) is legal, and the deck's colour identity is Clara's
chosen colour plus white and blue.
**Because:** CR 702.124m: Doctor's companion needs "a legendary Time Lord
Doctor creature card that has no other creature types"; the reminder text
"if the other is the Doctor" is shorthand for that. CR 702.124c,
CR 903.4: the identity is the two commanders' combined.
**Source:** [TR&CI 225: How Does Doctor's Companion Work (Or Not) w/ Doctor Strange, Doctor (And Other Doctors)](https://www.youtube.com/watch?v=FU2rQmOx0GQ)
**Engine:** partner is read as a keyword; Doctor's companion and Clara's
"choose a color before the game begins" are not, and `deck::read`
validating a pair is a separate question from the game.

### 2. A preparation card is only its creature for deck rules, so its prepare spell can be banned or duplicated

**Cards:** [Emeritus of Ideation // Ancestral Recall], [Grave Researcher // Reanimate], [Reanimate]
**Board:** Deck construction for Commander.
**Does:** P1's deck has Emeritus of Ideation, Grave Researcher and
Reanimate.
**Then:** It is legal: Ancestral Recall's ban doesn't touch Emeritus of
Ideation, and Grave Researcher and Reanimate have different names.
**Because:** CR 722.4: in every zone a preparation card has only its
normal characteristics, its name among them. CR 903.5b: singleton is by
English name. Scryfall reports Emeritus of Ideation legal in Commander.
**Source:** [TR&CI 218: Prepared Deep Dive - A Comprehensive Guide of 20+ Rules & Card Interactions w/ Prepared](https://www.youtube.com/watch?v=Hz8RnBvXXtQ)
**Engine:** deck construction, not the game: `scryfall check` and
`deck::read` would assert it.
