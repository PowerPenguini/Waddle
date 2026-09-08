#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Inject one external read failure (or process exit) after the target exists.
 * Enabled only for the child test process and one exact temporary file. */
static int inject(const char *path, int flags) {
    const char *target = getenv("WADDLE_AUDIT_TARGET");
    const char *armed = getenv("WADDLE_AUDIT_ARMED");
    int expected_access = getenv("WADDLE_AUDIT_WRITE") ? O_WRONLY : O_RDONLY;
    if (!target || !armed || (flags & O_ACCMODE) != expected_access || strcmp(path, target)) return 0;
    if (unlink(armed)) return 0;
    const char *mode = getenv("WADDLE_AUDIT_FAULT");
    if (mode && !strcmp(mode, "crash")) _exit(86);
    errno = EACCES;
    return 1;
}

#define WRAP_OPEN(name) \
int name(const char *path, int flags, ...) { \
    mode_t mode = 0; \
    if ((flags & O_CREAT) || ((flags & O_TMPFILE) == O_TMPFILE)) { \
        va_list args; va_start(args, flags); mode = va_arg(args, int); va_end(args); \
    } \
    int (*real_open)(const char *, int, ...) = dlsym(RTLD_NEXT, #name); \
    if (inject(path, flags)) return -1; \
    return real_open(path, flags, mode); \
}
WRAP_OPEN(open)
WRAP_OPEN(open64)
