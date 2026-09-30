# Ships the worker with the Nix-built site as its assets. Run by tofu
# (infra.nix) and by `nix run .#deploy-curator`.
{
  writeShellApplication,
  wrangler,
  worker,
  site,
}:
writeShellApplication {
  name = "deploy-curator";
  runtimeInputs = [wrangler];
  text = ''
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    # wrangler writes .wrangler/ next to its config, and the store is read-only
    cp -r ${worker}/src ${worker}/wrangler.toml "$work"
    cp -rL ${site} "$work/site"
    chmod -R u+w "$work"

    args=()
    # Secrets survive deploys, so one without it keeps whatever was set last
    if [ -n "''${GITHUB_CLIENT_SECRET:-}" ]; then
      (umask 077; printf 'GITHUB_CLIENT_SECRET=%s\n' "$GITHUB_CLIENT_SECRET" >"$work/secrets.env")
      args+=(--secrets-file secrets.env)
    fi
    cd "$work"
    wrangler deploy --config wrangler.toml "''${args[@]}" "$@"
  '';
}
