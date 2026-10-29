# Native Music Player

The owner selected a player compiled into Desktop-TUI instead of packaging the
mp3blaster executable. It replaces the pending external Music Player starter.

## Approaches

- Selected: a native AppCUI window and a compiled audio library. It shares the
  desktop's controls and needs no separately installed player.
- Alternative: distribute mp3blaster alongside the desktop. This preserves its
  interface but requires separate executable packaging for each platform.

## Behavior

Application > Utility > Music Player opens or restores one native window. Open
(Ctrl+O) selects one local MP3, WAV, FLAC, or Ogg Vorbis file. The player provides
Play/Pause (Space), Stop (S), ten-second seeking (Left/Right), and volume (+/-).
Show the filename, playback state, elapsed/total time, volume, and inline errors.
These controls and the first version's single-file scope are implementation defaults.

Hide and Terminal Lock preserve playback. Close and Desktop Exit stop playback
and release the audio device. Reopening creates a fresh worker. Include the typed
window handle in tiling, taskbar, theme behavior, and lock visibility restoration.
The native identity remains separate from any same-name custom external shortcut.

## Implementation constraints

Compile Symphonia 0.5.5 Rust decoders and a miniaudio device-only C bridge into the binary. Existing system
audio drivers remain necessary; missing devices produce an actionable inline
error. Building requires a C compiler, already available here.

Keep file reads, decoding, device setup, and seeking on one bounded worker per
window. Coalesce pending desired state instead of queueing unlimited actions.
Limit encoded input to 128 MiB, eight channels, and 8–192 kHz, and stream decoded samples. Preserve opaque OS
paths by reading through Rust and retaining encoded bytes for the stream lifetime.
Closing never waits in a UI callback and stale workers cannot update a new window.
Finish audio worker cleanup after the desktop event loop exits. Each native
device has its own handle and joins its callback before releasing Rust state.
A bounded PCM queue feeds a callback that never waits for decoding or seeking;
an atomic close flag silences it even if the worker is still busy.

No playlists, library indexing, network streams, or File Browser associations in
this version. Keep the optional mp3blaster example as a separately named shortcut.

## Evidence

Regression tests cover native dispatch/identity, controls and bounds, real codec
decoding, bad/oversized input, opaque paths, background updates, and close/reopen
races. Run the full Cargo checks and release build. Real-terminal playback and
device behavior require a manual audio check; headless tests do not establish it.
