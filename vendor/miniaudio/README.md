# miniaudio device backend

`miniaudio.h` is the single-header audio library distributed in
soloud-sys 1.1.1's miniaudio backend (miniaudio v0.10.42). Its copyright and license
notices are retained at the end of the header. Upstream:
<https://github.com/mackron/miniaudio>.

Desktop-TUI compiles it with decoding and the silent null backend disabled.
`src/runtime/audio_output.c` owns one device per handle and exposes only open,
render-callback, and close operations. Rust's Symphonia decodes local files.

Local patch: the ALSA wakeup eventfd reads and writes check their return values,
retry `EINTR`, and report failed or incomplete transfers. A failed capture wakeup
still attempts the playback wakeup. This fixes `-Wunused-result` diagnostics with
fortified glibc builds without disabling the warning.

Run the Linux regression without an audio device from the repository root:

```sh
cc -O2 -D_FORTIFY_SOURCE=2 -Werror=unused-result vendor/miniaudio/tests/alsa_wakeup.c -Wl,--wrap=read,--wrap=write -ldl -lpthread -lm -o /tmp/desktop-tui-alsa-wakeup-test
/tmp/desktop-tui-alsa-wakeup-test
```

It uses real eventfds and injected interrupted/failed/short transfers, and checks
that both capture and playback wakeups are signaled and drained. It does not
initialize ALSA or open a sound device. The linker must support GNU `--wrap`.

Check the production bridge with the same diagnostic enabled:

```sh
cc -O2 -D_FORTIFY_SOURCE=2 -Werror=unused-result -Ivendor/miniaudio -c src/runtime/audio_output.c -o /tmp/desktop-tui-audio-output.o
```
