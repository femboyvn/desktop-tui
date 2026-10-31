# Vendored terminal core

This directory contains the crates.io package
[`tattoy-wezterm-term` 0.1.0-fork.5](https://docs.rs/crate/tattoy-wezterm-term/0.1.0-fork.5),
from the [Tattoy WezTerm repository](https://github.com/tattoy-org/wezterm).
The package's `.cargo_vcs_info.json` records commit
`882ed5224937fb041595e33ce31f7daa955ea87a`, path `term`, and a dirty source tree.
The published package, rather than that commit alone, is the source of truth.
Its crate archive SHA-256 is
`7f3678674adc87f210ec01e2134e1972d6b0bd194bf82242f7e7beee3ae9dbd8`.
The original MIT license and Wez Furlong attribution remain in [LICENSE.md](LICENSE.md).
`Cargo.toml.orig` and `.cargo_vcs_info.json` are retained; the registry installation
marker and independent package lockfile are omitted. The application lockfile
pins the full dependency resolution. The application keeps `virtual-terminal`
for its Unix PTY implementation.

Local changes are confined to the integration boundary and print performer:

- Retain the last printed grapheme and its original line/cursor across byte-feed
  calls. If the next printable run extends it, restore that line and reprint the
  completed cluster immediately, with its updated display width. This covers
  combining marks, variation selectors, skin tones, flags and ZWJ sequences.
  Save only the final grapheme of a run; ordinary adjacent emoji stay separate.
  Interior graphemes retain one line. Margin-adjacent graphemes retain the
  active screen so a width change can undo wrapping or scrolling before
  reprinting. Wide graphemes wrap before placement when they cannot fit.
  Controls and resize invalidate the retained anchor; insert mode restores the
  line before insertion. UTF-8 decoding remains the upstream streaming parser.
- Add an opt-out of the detached threaded writer. Desktop-TUI uses a synchronous
  memory writer bounded to 64 KiB so protocol replies and mouse bytes are
  available in the same callback. An exhaustion flag lets the owner report the
  failure and close the child. PTY writes remain outside the emulator.
- Add a text-only image-protocol configuration hook. Desktop-TUI ignores Sixel,
  Kitty and iTerm image actions and does not advertise Sixel or absent clipboard
  access in primary device attributes. XTSMGRAPHICS queries report unsupported
  items. No image escape sequence reaches AppCUI.
- Expose the application-cursor mode and cursor-visibility type for the facade.
  A terminal-name hook makes XTGETTCAP TN agree with `TERM=xterm-256color`, while
  identifying the host application as Desktop-TUI.

The facade in `src/runtime/terminal.rs` owns preferences palette projection,
complete grapheme payloads, width/continuation cells, bounded protocol replies,
mode-aware paste and mouse encoding. Explicit RGB colors retain their value;
default and indexed ANSI colors follow preference changes. Unicode width uses
version 14 with narrow ambiguous characters. Scrollback is disabled for this
embedded viewport. Kitty keyboard/graphics stay disabled. Style projection
supports bold, italic, underline variants, strikethrough, reverse and invisible;
AppCUI has no dim or blinking-cell attribute.

Compatibility was checked first with the unmodified exact package in an isolated
Cargo project using rustc 1.97.1. A regression feeding `a`, then U+0301 failed on
the unmodified package and passes with this patch. Application tests cover all
byte split points, scalar-at-a-time feeds, long combining runs, adjacent emoji,
wide continuations, wrapping, insert/erase, resize, alt screen, scroll regions,
colors, cursor visibility, query replies, paste and mouse modes. Run them with:

```sh
cargo test --locked runtime::terminal::tests
```

Real-terminal host width/font behavior, PTY programs and visual redraw during
move/resize still require the manual checks in `HOW_TO_TEST.md`.
