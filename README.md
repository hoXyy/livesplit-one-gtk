# LiveSplit One GTK

An **unofficial** Linux desktop version of LiveSplit One built with Relm4, GTK 4, libadwaita,
and [livesplit-core](https://github.com/LiveSplit/livesplit-core). Based on [AlexKnauth's version of livesplit-one-druid](https://github.com/AlexKnauth/livesplit-one-druid).

Available as a package:
- AUR: [livesplit-one-gtk-git](https://aur.archlinux.org/packages/livesplit-one-gtk-git)
- Nix: see [below](#nix)

## Nix

The package builds from source, from the `main` branch.

### Without flakes

Install it into your user profile:

```bash
nix-env -if https://github.com/hoXyy/livesplit-one-gtk/archive/main.tar.gz
```

Or add it to your NixOS (or Home Manager) configuration:

```nix
{ pkgs, ... }:
let
  livesplit-one-gtk = import (builtins.fetchTarball
    "https://github.com/hoXyy/livesplit-one-gtk/archive/main.tar.gz") { };
in
{
  environment.systemPackages = [ livesplit-one-gtk ];
  # Home Manager: home.packages = [ livesplit-one-gtk ];
}
```

This builds against the nixpkgs revision pinned in this repository's `flake.lock`,
so it works whatever channel you're on. To build against your own nixpkgs instead,
use `import (...) { inherit pkgs; }`. That needs a channel whose Rust is new enough for
the dependencies (currently nixos-unstable).

From a clone of this repository, `nix-build` builds it into `./result`.

### With flakes

Run it without installing:

```bash
nix run github:hoXyy/livesplit-one-gtk
```

Install it into your user profile:

```bash
nix profile install github:hoXyy/livesplit-one-gtk
```

Or add it to your system flake:

```nix
{
  inputs.livesplit-one-gtk.url = "github:hoXyy/livesplit-one-gtk";

  outputs = { nixpkgs, livesplit-one-gtk, ... }: {
    nixosConfigurations.<hostname> = nixpkgs.lib.nixosSystem {
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            livesplit-one-gtk.packages.${pkgs.stdenv.hostPlatform.system}.default
          ];
        })
      ];
    };
  };
}
```

An overlay is also available as `livesplit-one-gtk.overlays.default`.

### Letting autosplitters read game memory on NixOS

The `setcap` command [below](#note-about-autosplitters) doesn't work on files in the
Nix store. Instead, add a security wrapper that gives LiveSplit `CAP_SYS_PTRACE`:

```nix
security.wrappers.livesplit-one-gtk = {
  source = lib.getExe livesplit-one-gtk; # the package from one of the snippets above
  owner = "root";
  group = "root";
  capabilities = "cap_sys_ptrace+p";
};
```

The wrapper is installed as `/run/wrappers/bin/livesplit-one-gtk`, which comes first
on `PATH`, so the desktop entry and the `livesplit-one-gtk` command both go through
it. Only LiveSplit (and anything it starts) gets the capability. Autosplitters then
work with any `kernel.yama.ptrace_scope` setting except 3, which blocks memory
access for everyone.

> **Security note:** With the wrapper, autosplitters can read the memory of every
> process running as your account, not just the game.

## Build requirements

- Rust stable
- GTK 4.14 or newer
- libadwaita 1.5 or newer
- X11 development libraries for the X11 placement backend

Build and test with:

```bash
cargo build --release --locked
cargo test --all-features
```

### AppImage

Every push to `main` publishes an AppImage on the
[releases page](https://github.com/hoXyy/livesplit-one-gtk/releases). Updaters that
read AppImage update information, such as Gear Lever, can find new builds on their own.

To build one yourself, run `packaging/appimage/build.sh` on Ubuntu 24.04, or in a
container of it. The script lists the packages it needs, and the result ends up in
`target/appimage`.

## A note about global hotkeys on Wayland

On Wayland, LiveSplit's global hotkeys use Linux evdev input devices so that
keyboard and controller shortcuts work while another application is focused.
If LiveSplit cannot access those devices, it starts normally with global
hotkeys disabled. Timer controls and shortcut configuration remain available.

> **Security warning:** The following change allows every program running as
> your account to read raw keyboard and controller input, potentially including
> passwords and other sensitive keystrokes. It does not grant access only to
> LiveSplit. Leave global hotkeys disabled if you do not understand or accept
> this risk.

If you're fine with the above, enter this command in a
terminal:

```bash
sudo usermod -aG input "$USER"
```

This adds your account to the `input` group, allowing apps running under your account to have direct access to input devices.

Sign out and sign back in, then launch LiveSplit. Closing LiveSplit or opening a new terminal is not enough.
Verify that `input` appears as a separate group name with:

```bash
id -nG
```

To remove the permission, use:

```bash
sudo gpasswd -d "$USER" input
```

Then fully sign out and back in again.

If the system has no `input` group, consult your distribution's input-device
permission documentation.

## Configuration location

Configuration is stored here:

```text
~/.local/share/livesplitone/config.yml
```

# Note about autosplitters

Autosplitters may require `CAP_SYS_PTRACE` to be able to read other processes's memory:

```bash
sudo setcap CAP_SYS_PTRACE=+eip /usr/bin/livesplit-one-gtk
```

This doesn't work with the AppImage, because capabilities can't be set on a binary
inside it. The AppImage has to rely on what the system's `kernel.yama.ptrace_scope`
setting allows instead.

# Official versions of LiveSplit One

The official versions of LiveSplit One are:
- [OBS Plugin](https://github.com/livesplit/obs-livesplit-one)
- [Web version](https://one.livesplit.org/)
