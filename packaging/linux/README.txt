Desktop-TUI with Carboxyl - Linux x86_64 bundle

This package includes two executables: Desktop-TUI and the unmodified Carboxyl
0.2.0 browser. Keep desktop-tui and libexec/carboxyl together when moving it.
The browser is found relative to the Desktop-TUI executable, so no separate
Carboxyl installation or PATH modification is needed.

HOST REQUIREMENTS
The upstream Carboxyl binary requires GNU libc (glibc) 2.43 or newer and
libstdc++.so.6 providing GLIBCXX_3.4.30. It also needs zlib, fontconfig, libgcc,
the GNU dynamic loader, their runtime dependencies, and usable system fonts.
Ubuntu 26.04 was checked for command-line loading. Older distributions may not
meet these requirements. This bundle carries the apps, not a full Linux runtime.
On an older distribution, build Carboxyl from the pinned source tag for that
system and use a source-built Desktop-TUI with that browser on PATH instead.

Use a real terminal with true-color support. Desktop-TUI's native Music Player
uses the host audio device/session. Other sample shortcuts need their named
programs installed separately. The PDF Reader needs Poppler's pdfinfo and
pdftoppm on PATH. Nothing in this package installs or modifies host software.

VERIFY AND UNPACK
Put the archive and its SHA256SUMS in the same directory, then run:

    sha256sum -c SHA256SUMS
    tar -xzf desktop-tui-<version>-linux-x86_64.tar.gz
    cd desktop-tui-<version>-linux-x86_64
    ./desktop-tui

Replace <version> with the package filename's version. The default starter apps
include Application > Utility > Web Browser, which opens the bundled Carboxyl
with --graphics quadrant --fps 20. No configuration is copied on first launch.
Settings still use ~/.config/desktop-tui (or the absolute XDG_CONFIG_HOME).

To load the included example shortcuts explicitly, run:

    ./desktop-tui ./shortcuts

The examples include programs not bundled here and an intentional missing-command
example. From another working directory use absolute paths for the executable
and, if requested, the shortcut directory. Paths containing spaces should be
quoted. Launching with --help checks CLI startup only; use a real terminal to
verify browser navigation and the desktop UI.

COMPATIBILITY AND REMOVAL
A shortcut using the bare command carboxyl selects libexec/carboxyl when it is
an executable regular file. Explicit paths and other commands keep their usual
meaning. Desktop-TUI installations without this sidecar fall back to PATH.
To remove the bundle, delete its unpacked directory. Saved settings remain in
your configuration directory until you choose to remove them.

LICENSES AND SOURCE
Desktop-TUI: licenses/Desktop-TUI-LICENSE (MIT)
https://github.com/femboyvn/desktop-tui

The compiled terminal core's MIT license and copyright are retained in
licenses/WezTerm-LICENSE. AppCUI's retained package metadata, including author,
license and source, is in licenses/AppCUI-Cargo.toml. The local dependencies'
provenance and patch record are in licenses/Local-dependencies.md.

Carboxyl: licenses/Carboxyl-LICENSE (BSD-3-Clause)
Release, source, pinned archive digest, and inspected runtime requirements are in
licenses/Carboxyl-PROVENANCE.json.
https://github.com/carboxyl-rs/carboxyl/tree/v0.2.0
https://github.com/carboxyl-rs/carboxyl/releases/tag/v0.2.0

BUILDING THE BUNDLE FROM SOURCE
From the Desktop-TUI repository, with Python 3.11+ and Rust/Cargo installed:

    python3 scripts/package-linux.py

This runs cargo build --locked --release and downloads the pinned browser release
into target/package-cache. A SHA-256 mismatch stops packaging. Downloads and
package files use temporary .part files before atomic replacement. Output is
target/dist/desktop-tui-<version>-linux-x86_64.tar.gz and target/dist/SHA256SUMS.

To reuse a build and an already downloaded browser archive:

    python3 scripts/package-linux.py --desktop-binary target/release/desktop-tui --carboxyl-archive /path/to/carboxyl-v0.2.0-x86_64-linux.tar.gz

Use --output-dir /path/to/output to choose the destination. Both executables are
checked for Linux x86_64 ELF headers. Packaging does not run the apps, install
libraries, or verify the target machine's runtime. Repeated packaging with the
same input contents produces identical archives (normalized order, modes,
ownership, timestamps, and gzip header).
