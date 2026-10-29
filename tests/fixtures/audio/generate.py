"""Regenerate the synthetic 0.2-second, 440 Hz test tones using libsndfile."""
import ctypes as c
import math
from pathlib import Path

class Info(c.Structure):
    _fields_ = [("frames", c.c_longlong), ("samplerate", c.c_int),
                ("channels", c.c_int), ("format", c.c_int),
                ("sections", c.c_int), ("seekable", c.c_int)]

snd = c.CDLL("libsndfile.so.1")
snd.sf_open.argtypes = [c.c_char_p, c.c_int, c.POINTER(Info)]
snd.sf_open.restype = c.c_void_p
snd.sf_writef_short.argtypes = [c.c_void_p, c.POINTER(c.c_short), c.c_longlong]
snd.sf_writef_short.restype = c.c_longlong
snd.sf_close.argtypes = [c.c_void_p]
snd.sf_strerror.argtypes = [c.c_void_p]
snd.sf_strerror.restype = c.c_char_p
samples = (c.c_short * 8820)(*(int(1000 * math.sin(2 * math.pi * 440 * n / 44100)) for n in range(8820)))
for extension, encoding in [("wav", 0x010002), ("flac", 0x170002), ("ogg", 0x200060), ("mp3", 0x230082)]:
    path = Path(__file__).parent / f"tone.{extension}"
    info = Info(0, 44100, 1, encoding, 0, 0)
    handle = snd.sf_open(bytes(path), 0x20, c.byref(info))
    if not handle:
        raise RuntimeError(snd.sf_strerror(None).decode())
    try:
        assert snd.sf_writef_short(handle, samples, len(samples)) == len(samples)
    finally:
        assert snd.sf_close(handle) == 0
    print(path.name, path.stat().st_size)
