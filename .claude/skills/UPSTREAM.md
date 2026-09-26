# Matt Pocock's skills, vendored

The skill folders here are copied verbatim from
[mattpocock/skills](https://github.com/mattpocock/skills) (MIT, see
`LICENSE-mattpocock-skills`): exactly the 25 its `.claude-plugin/plugin.json`
lists, flattened out of `skills/engineering/` and `skills/productivity/`.

Source commit: `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` (2026-09-18).

They are vendored rather than enabled as a plugin because Claude Code cloud
sessions do not install plugins from a project's `extraKnownMarketplaces`,
while `.claude/skills/` is read straight from the checkout.

To refresh, from the repo root:

```
git clone --depth 1 https://github.com/mattpocock/skills /tmp/mp
for d in $(jq -r '.skills[]' /tmp/mp/.claude-plugin/plugin.json); do
  rm -rf ".claude/skills/$(basename "$d")" && cp -r "/tmp/mp/$d" .claude/skills/
done
cp /tmp/mp/LICENSE .claude/skills/LICENSE-mattpocock-skills
```

then update the commit above. A skill dropped upstream has to be deleted here
by hand.
