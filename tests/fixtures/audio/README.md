# Synthetic audio fixtures

These files encode a generated 0.2-second, 440 Hz sine wave at 44.1 kHz mono.
They contain no recorded or third-party music. They are provided under this
repository's MIT license and test the bundled MP3, WAV, FLAC, and Ogg Vorbis
decoders without requiring an audio output device.

`python3 tests/fixtures/audio/generate.py` regenerates them using the host's
libsndfile (including MP3 encoding support). Regeneration is optional; normal
Cargo tests read the committed fixtures and require no libsndfile installation.
