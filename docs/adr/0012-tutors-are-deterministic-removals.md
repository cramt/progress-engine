# A tutor is a deterministic removal; exiling off the top is a draw

The library can shrink without being drawn from ([#18](https://github.com/cramt/progress-engine/issues/18)). A tutor removes one card from a named group, and given the checkpoints so far there is only one answer. It is therefore a subtraction from the pool the next gap is dealt from, and costs no enumeration width. Exiling off the top is a random sample, so it branches the path the way a draw does. That half is filed, not approximated.

`chip-stats` stays Magic-free. It gained one vocabulary word, **removals**, beside populations, groups and draws. The condition #18 set: if the signature ever needs to know what a tutor is, the abstraction is wrong.

See [VISION.md: The library is not a fixed population](../../VISION.md#the-library-is-not-a-fixed-population).
