# Native Music Player Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Play local audio inside Desktop-TUI without installing mp3blaster.

**Architecture:** A native AppCUI window sends coalesced desired playback state
to one audio worker. The worker owns the Rust decoder and miniaudio output device and
publishes bounded snapshots; desktop code owns a typed window handle.

**Tech Stack:** Rust 2024, AppCUI, Symphonia 0.5.5, miniaudio, C compiler.

**Spec:** [Native Music Player](../specs/2026-10-29-native-music-player-design.md)

## Global constraints

- Local MP3, WAV, FLAC, and Ogg Vorbis; maximum encoded input 128 MiB, eight channels, 8–192 kHz.
- Space Play/Pause, S Stop, Left/Right seek ten seconds, +/- volume, Ctrl+O Open.
- No blocking I/O, decoding, device setup, or seeking in UI callbacks.
- Hide and Terminal Lock preserve playback; Close and Exit release resources.
- Preserve shortcut schema and keep native/custom application identities distinct.

## Review focus

- Close during a pending load must prevent late playback and stale UI updates.
- Audio-device failure must leave the window usable and allow opening again.
- Non-UTF-8 and placeholder-like filenames must reach Rust file I/O unchanged.
- Repeated controls must stay bounded and preserve the most recent desired state.
- Hidden or locked players must keep playing without opening error dialogs.

## Tasks

- [x] Catalog: replace the external starter with `AppId::MusicPlayer`; test
  empty-folder availability and independence from same-name custom shortcuts.
- [ ] Runtime (`src/runtime/music.rs`): implement bounded input loading, retained
  encoded stream storage, a coalescing worker/controller, status snapshots, and
  idempotent shutdown. Test actual decoding plus controls, bounds, errors, and
  blocked-load close/reopen races without requiring a physical audio device.
- [ ] UI (`src/ui/native/music_player.rs`): implement the native window, inline
  status, buttons/keys, picker, and timer polling. Test controls on small surfaces.
- [x] Desktop: add the typed handle to launch/focus/hide/close, taskbar, tiling,
  and Terminal Lock paths; finish worker shutdown from `main`. Extend lifecycle
  tests with repeated launch and close/reopen.
- [x] Documentation and optional external example: describe bundled playback,
  formats/controls/limits and C compiler build prerequisites; rename the example's
  display name to mp3blaster so it coexists with the native player.
- [x] Verify: targeted regressions followed by check, test, Clippy, format check,
  whitespace check, release build, and an independent final source review. Report
  real-terminal audio checks as unperformed when the environment cannot run them.

Leave changes local and uncommitted, preserving the existing task branch.

## Implementation rulings and evidence

- Replaced the originally evaluated SoLoud decoder after an independent headless
  probe reproduced crashes in its MP3/FLAC streaming seek path. The final design
  uses Symphonia for all decoding/seeking; miniaudio only owns output devices.
- Added codec pre-roll to reconstruct MP3 state after seeks. Real decoded-sample
  tests exercise initial, forward, and backward seeking for all four formats.
- Review found that stale control updates reset playback position and that output
  needed channel identities. Regression tests now preserve position across a stale
  volume update and preserve mono/stereo/2.1/quad/7.1 channel maps.
- The final source review found both issues resolved. Audible output and
  real-terminal behavior remain manual checks; no interactive desktop was launched.

Final automated evidence: 105 tests passed; Cargo check, Clippy, format check,
whitespace check, and release build passed. Clippy retains 19 binary / 24 test
warnings in existing Rust code; release compilation reports upstream miniaudio
wake-pipe read/write return-value warnings. Manual audio/terminal checks remain.
