#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
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
    const char *effect_missing = getenv("WADDLE_AUDIT_EFFECT_MISSING");
    if (effect_missing && access(effect_missing, F_OK) == 0) return 0;
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

/* Interrupt a cross-device Move after a specific source entry was unlinked. */
int unlink(const char *path) {
    int (*real_unlink)(const char *) = dlsym(RTLD_NEXT, "unlink");
    int result = real_unlink(path);
    const char *target = getenv("WADDLE_AUDIT_UNLINK");
    const char *armed = getenv("WADDLE_AUDIT_ARMED");
    if (!result && target && armed && !strcmp(path, target) && !real_unlink(armed)) _exit(86);
    return result;
}

/* Stop after the write-ahead journal rename, before result publication. */
int rename(const char *from, const char *to) {
    int (*real_rename)(const char *, const char *) = dlsym(RTLD_NEXT, "rename");
    int (*real_unlink)(const char *) = dlsym(RTLD_NEXT, "unlink");
    int result = real_rename(from, to);
    const char *commit = getenv("WADDLE_AUDIT_COMMIT");
    const char *armed = getenv("WADDLE_AUDIT_ARMED");
    if (!result && commit && armed && !strcmp(to, commit) && !real_unlink(armed)) _exit(86);
    return result;
}

/* Fail data synchronization only for a file below the isolated test target. */
int fsync(int fd) {
    int (*real_fsync)(int) = dlsym(RTLD_NEXT, "fsync");
    const char *target = getenv("WADDLE_AUDIT_SYNC_TARGET");
    const char *armed = getenv("WADDLE_AUDIT_ARMED");
    if (target && armed) {
        char descriptor[64], path[4096];
        int length = snprintf(descriptor, sizeof descriptor, "/proc/self/fd/%d", fd);
        if (length > 0) {
            ssize_t count = readlink(descriptor, path, sizeof path - 1);
            if (count > 0) {
                path[count] = 0;
                size_t prefix = strlen(target);
                int exact = getenv("WADDLE_AUDIT_SYNC_EXACT") != NULL;
                if (exact ? !strcmp(path, target) : (!strncmp(path, target, prefix) && path[prefix] == '/')) {
                    int (*real_unlink)(const char *) = dlsym(RTLD_NEXT, "unlink");
                    real_unlink(armed);
                    const char *error = getenv("WADDLE_AUDIT_SYNC_ERRNO");
                    errno = error ? atoi(error) : EIO;
                    return -1;
                }
            }
        }
    }
    return real_fsync(fd);
}
