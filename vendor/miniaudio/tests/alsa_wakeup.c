/* Hardware-free regression for the local ALSA eventfd patch. */
#define MINIAUDIO_IMPLEMENTATION
#define MA_ENABLE_ONLY_SPECIFIC_BACKENDS
#define MA_ENABLE_ALSA
#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_NULL
#include "../miniaudio.h"
#include <assert.h>

static int fault_fd = -1;
static int read_interrupts;
static int write_interrupts;
static int read_error;
static int write_error;
static int short_read;
static int short_write;

ssize_t __real_read(int fd, void* data, size_t size);
ssize_t __real_write(int fd, const void* data, size_t size);

ssize_t __wrap_read(int fd, void* data, size_t size)
{
    if (fd == fault_fd) {
        if (read_interrupts > 0) { read_interrupts -= 1; errno = EINTR; return -1; }
        if (read_error != 0) { errno = read_error; return -1; }
        if (short_read) { return 4; }
    }
    return __real_read(fd, data, size);
}

ssize_t __wrap_write(int fd, const void* data, size_t size)
{
    if (fd == fault_fd) {
        if (write_interrupts > 0) { write_interrupts -= 1; errno = EINTR; return -1; }
        if (write_error != 0) { errno = write_error; return -1; }
        if (short_write) { return 4; }
    }
    return __real_write(fd, data, size);
}

static int ready(int fd)
{
    struct pollfd descriptor = {fd, POLLIN, 0};
    return poll(&descriptor, 1, 0);
}

static ma_result consume_wakeup(ma_device* device, int fd)
{
    struct pollfd descriptor = {fd, POLLIN, 0};
    assert(ready(fd) == 1); /* Keep a broken wakeup from blocking this test. */
    return ma_device_wait__alsa(device, NULL, &descriptor, 1, POLLIN);
}

int main(void)
{
    ma_context context;
    ma_device device;
    struct pollfd capture;
    struct pollfd playback;
    ma_uint64 value = 1;
    MA_ZERO_OBJECT(&context);
    MA_ZERO_OBJECT(&device);
    device.pContext = &context;
    capture.fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
    playback.fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
    assert(capture.fd >= 0 && playback.fd >= 0);
    device.alsa.pPollDescriptorsCapture = &capture;
    device.alsa.pPollDescriptorsPlayback = &playback;
    device.alsa.wakeupfdCapture = capture.fd;
    device.alsa.wakeupfdPlayback = playback.fd;

    assert(ma_device_data_loop_wakeup__alsa(&device) == MA_SUCCESS);
    assert(consume_wakeup(&device, capture.fd) == MA_DEVICE_NOT_STARTED);
    assert(consume_wakeup(&device, playback.fd) == MA_DEVICE_NOT_STARTED);
    assert(ready(capture.fd) == 0 && ready(playback.fd) == 0);

    fault_fd = capture.fd;
    write_interrupts = 1;
    read_interrupts = 1;
    assert(ma_device_data_loop_wakeup__alsa(&device) == MA_SUCCESS);
    assert(consume_wakeup(&device, capture.fd) == MA_DEVICE_NOT_STARTED);
    assert(consume_wakeup(&device, playback.fd) == MA_DEVICE_NOT_STARTED);
    assert(ready(capture.fd) == 0 && read_interrupts == 0 && write_interrupts == 0);

    write_error = EBADF;
    assert(ma_device_data_loop_wakeup__alsa(&device) == ma_result_from_errno(EBADF));
    assert(consume_wakeup(&device, playback.fd) == MA_DEVICE_NOT_STARTED);
    write_error = 0;
    short_write = 1;
    assert(ma_device_data_loop_wakeup__alsa(&device) == MA_IO_ERROR);
    assert(consume_wakeup(&device, playback.fd) == MA_DEVICE_NOT_STARTED);
    short_write = 0;

    assert(__real_write(capture.fd, &value, sizeof(value)) == sizeof(value));
    read_error = EIO;
    assert(consume_wakeup(&device, capture.fd) == ma_result_from_errno(EIO));
    read_error = 0;
    short_read = 1;
    assert(consume_wakeup(&device, capture.fd) == MA_IO_ERROR);
    short_read = 0;
    assert(consume_wakeup(&device, capture.fd) == MA_DEVICE_NOT_STARTED);
    assert(ready(capture.fd) == 0);

    assert(close(capture.fd) == 0);
    assert(close(playback.fd) == 0);
    return 0;
}
