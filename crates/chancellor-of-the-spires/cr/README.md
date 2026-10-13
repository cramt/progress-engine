# chancellor-cr

The Comprehensive Rules the repo is built against, pinned by hash in
[pin.json](pin.json). The text is Wizards' and the repo is public, so it is
never committed: the flake fetches it by that hash, and `cr` keeps it in
`~/.cache/chancellor/` (or reads `CHANCELLOR_CR`), checking the hash either way.

```
cargo run -p chancellor-cr -- check            # every "CR nnn.n" in the repo names a rule in the pinned CR
cargo run -p chancellor-cr -- diff <URL|FILE>  # a new CR against the pinned one, as a worklist
cargo run -p chancellor-cr -- pin <URL>        # rewrite pin.json to a new CR
```

`nix flake check` runs `check` as `cr-citations`, over the whole tree, docs
included. A citation is "CR " and a number: a section (`613`), a rule
(`613.4`), a subrule (`613.4c`), a range (`102.2–102.3`, `613.8a–c`) or a list
(`107.3a, 107.3d`). Citing a section or a rule cites what is under it.

## When Wizards publishes a new CR

1. `cr diff <new URL> > worklist.md`. It lists every rule that changed, was
   removed or was renumbered and is cited somewhere, old text against new,
   with each place that cites it; then the changes nothing cites, and the
   added rules (new mechanics Insight may not read yet).
2. Hand the worklist to an agent, as an issue. It says what to do: recheck
   each citing place against the new text, fix behaviour, tests or claims
   that relied on what changed, update renumbered citations.
3. The same change runs `cr pin <new URL>` and leaves `cr check` clean, so
   the pin moves in the commit that answers for it.
