# Non-flake entry point.
#
# By default this builds against the nixpkgs revision pinned in flake.lock, so it
# works no matter which channel you're on. Pass `pkgs` to use your own nixpkgs
# instead (it needs a Rust toolchain new enough for the dependencies):
#
#   import ./. { inherit pkgs; }
{
  pkgs ?
    let
      lock = (builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.nixpkgs.locked;
    in
    import (fetchTarball {
      url = "https://github.com/NixOS/nixpkgs/archive/${lock.rev}.tar.gz";
      sha256 = lock.narHash;
    }) { },
}:

pkgs.callPackage ./nix/package.nix { }
