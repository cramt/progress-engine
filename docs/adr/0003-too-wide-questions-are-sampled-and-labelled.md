# A question too wide to enumerate is sampled, and says so

A question above `MAX_PATHS` (five million compositions) used to be refused (`RunError::TooWide`). It now falls back to the sampler ([#48](https://github.com/cramt/progress-engine/issues/48), `29a9c76`), because the intended caller is a graphical builder that cannot pass a flag or act on advice to ask something smaller. What makes this acceptable is the labelling: an estimate cannot be mistaken for an exact answer. The run says above its numbers which figures were estimated; every sampled figure carries its error bar; the JSON says per answer which engine produced it and why. `--exact` restores the refusal.

A silent fallback was rejected, because a number quietly changing kind is the failure this tool exists to prevent.

See [VISION.md: Never answer a question you did not model](../../VISION.md#never-answer-a-question-you-did-not-model).
