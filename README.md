# LiveSplit One GTK

An **unofficial** Linux desktop version of LiveSplit One built with Relm4, GTK 4, libadwaita,
and [livesplit-core](https://github.com/LiveSplit/livesplit-core). Based on [AlexKnauth's version of livesplit-one-druid](https://github.com/AlexKnauth/livesplit-one-druid).

Available as a package:
- AUR: [livesplit-one-gtk-git](https://aur.archlinux.org/packages/livesplit-one-gtk-git)

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
