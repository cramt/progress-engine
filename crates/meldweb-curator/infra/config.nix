# Names the OpenTofu stack needs that wrangler.toml doesn't already hold. The
# worker's name and account come from wrangler.toml itself (see flake.nix), so
# `wrangler dev`, a manual deploy and tofu can't disagree about them.
{
  zone = "cramt.dk";
  hostname = "meldweb.cramt.dk";
  # Created by hand once: tofu can't create the bucket that holds its own state
  stateBucket = "cramt-tofu-state";
}
