{
  description = "Headless harness and API for the Delver X card-recognition engine";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            nodejs_24      # .fixtures/fetch-cards.sh parses Scryfall JSON with it
            rustc          # the deno_core host
            cargo
            rustfmt
            clippy
            p7zip          # fetch.sh unpacks Delver's .7z blobs
            imagemagick    # examples and tests decode card images to raw RGBA
            wasm-tools     # validating the tag patch
            curl
          ];

          # rusty_v8 downloads a prebuilt libv8 rather than building V8 from
          # source; without this it looks for a sysroot it has no reason to find.
          env.RUSTY_V8_MIRROR = "https://github.com/denoland/rusty_v8/releases/download";
        };
      });
    };
}
