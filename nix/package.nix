{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  glib,
  cairo,
  pango,
  gdk-pixbuf,
  graphene,
  aws-lc,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "livesplit-one-gtk";
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
      ../src
      ../resources
      ../icons
      ../packaging/org.livesplit.LiveSplitOne.desktop
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    # Fetches git dependencies (livesplit-core) by the revision in Cargo.lock,
    # so there are no extra hashes to update when it changes.
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    gtk4
    libadwaita
    glib
    cairo
    pango
    gdk-pixbuf
    graphene
    # nixpkgs makes aws-lc-sys (rustls, via livesplit-core) link the system library.
    aws-lc
  ];

  postInstall = ''
    install -Dm644 packaging/org.livesplit.LiveSplitOne.desktop \
      $out/share/applications/org.livesplit.LiveSplitOne.desktop
    install -Dm644 icons/icon.svg \
      $out/share/icons/hicolor/scalable/apps/org.livesplit.LiveSplitOne.svg
    install -Dm644 icons/icon.png \
      $out/share/pixmaps/org.livesplit.LiveSplitOne.png
  '';

  meta = {
    description = "Linux desktop version of LiveSplit One using GTK4 and libadwaita";
    homepage = "https://github.com/hoXyy/livesplit-one-gtk";
    license = lib.licenses.mit;
    mainProgram = "livesplit-one-gtk";
    platforms = lib.platforms.linux;
  };
}
