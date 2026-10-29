# miniaudio device backend

`miniaudio.h` is the unmodified single-header audio library distributed in
soloud-sys 1.1.1's miniaudio backend (miniaudio v0.10.42). Its copyright and license
notices are retained at the end of the header. Upstream:
<https://github.com/mackron/miniaudio>.

Desktop-TUI compiles it with decoding and the silent null backend disabled.
`src/runtime/audio_output.c` owns one device per handle and exposes only open,
render-callback, and close operations. Rust's Symphonia decodes local files.
