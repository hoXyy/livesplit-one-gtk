{
  description = "Linux desktop version of LiveSplit One using GTK4 and libadwaita";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        livesplit-one-gtk = pkgs.callPackage ./nix/package.nix { };
        default = livesplit-one-gtk;
      });

      overlays.default = final: _prev: {
        livesplit-one-gtk = final.callPackage ./nix/package.nix { };
      };
    };
}
