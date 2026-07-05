#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static const size_t kPageSize = 4096;
static const size_t kBytesPerMb = 1024 * 1024;
static volatile uint64_t g_sink = 0;

static long parse_positive(const char *raw, const char *name) {
    char *end = NULL;
    errno = 0;
    long value = strtol(raw, &end, 10);
    if (errno != 0 || end == raw || *end != '\0' || value <= 0) {
        fprintf(stderr, "%s must be a positive integer, got '%s'\n", name, raw);
        exit(2);
    }
    return value;
}

static void sleep_seconds(long seconds) {
    struct timespec remaining;
    remaining.tv_sec = seconds;
    remaining.tv_nsec = 0;
    while (nanosleep(&remaining, &remaining) != 0) {
        if (errno != EINTR) {
            perror("nanosleep");
            exit(3);
        }
    }
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <hold_mb> <hold_secs>\n", argv[0]);
        return 2;
    }

    long hold_mb = parse_positive(argv[1], "hold_mb");
    long hold_secs = parse_positive(argv[2], "hold_secs");
    size_t bytes = (size_t)hold_mb * kBytesPerMb;
    if (bytes / kBytesPerMb != (size_t)hold_mb) {
        fprintf(stderr, "hold_mb is too large\n");
        return 2;
    }

    uint8_t *buffer = (uint8_t *)malloc(bytes);
    if (buffer == NULL) {
        fprintf(stderr, "malloc(%zu) failed: %s\n", bytes, strerror(errno));
        return 1;
    }

    volatile uint8_t *resident = (volatile uint8_t *)buffer;
    for (size_t offset = 0; offset < bytes; offset += kPageSize) {
        resident[offset] = 1;
        g_sink += resident[offset];
    }
    resident[bytes - 1] = 1;
    g_sink += resident[bytes - 1];

    printf("holding %ld MB for %ld seconds (sink=%llu)\n",
           hold_mb,
           hold_secs,
           (unsigned long long)g_sink);
    fflush(stdout);
    sleep_seconds(hold_secs);

    free(buffer);
    return 0;
}
