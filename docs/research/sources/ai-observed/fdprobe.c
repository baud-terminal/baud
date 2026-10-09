#include <stdio.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/param.h>
#include <unistd.h>
#include <string.h>
#include <stdlib.h>
#include <errno.h>
#include <os/log.h>
static void describe(FILE *out, int fd) {
    struct stat st;
    if (fstat(fd, &st) != 0) { fprintf(out, "fd %d: fstat failed errno=%d (%s)\n", fd, errno, strerror(errno)); return; }
    const char *kind = S_ISCHR(st.st_mode) ? "chr" : S_ISREG(st.st_mode) ? "reg" : S_ISFIFO(st.st_mode) ? "fifo" : S_ISSOCK(st.st_mode) ? "sock" : "other";
    char path[MAXPATHLEN] = "";
    int r = fcntl(fd, F_GETPATH, path);
    fprintf(out, "fd %d: kind=%s dev=%d rdev=%d ino=%llu path=%s (F_GETPATH r=%d) isatty=%d\n", fd, kind, (int)st.st_dev, (int)st.st_rdev, (unsigned long long)st.st_ino, r==0?path:"?", r, isatty(fd));
}
int main(int argc, char **argv) {
    const char *outp = getenv("HOME");
    char p[1024];
    snprintf(p, sizeof p, "%s", "<scratch>/scratchpad/probe/result.txt");
    FILE *out = fopen(p, "a");
    if (!out) return 2;
    fprintf(out, "--- pid=%d ppid=%d argv0=%s\n", getpid(), getppid(), argv[0]);
    for (int fd = 0; fd < 3; fd++) describe(out, fd);
    struct stat devnull; stat("/dev/null", &devnull);
    fprintf(out, "/dev/null: dev=%d rdev=%d ino=%llu\n", (int)devnull.st_dev, (int)devnull.st_rdev, (unsigned long long)devnull.st_ino);
    fprintf(out, "env TERM=%s LANG=%s PATH=%s XPC_SERVICE_NAME=%s\n", getenv("TERM")?getenv("TERM"):"(unset)", getenv("LANG")?getenv("LANG"):"(unset)", getenv("PATH")?getenv("PATH"):"(unset)", getenv("XPC_SERVICE_NAME")?getenv("XPC_SERVICE_NAME"):"(unset)");
    fprintf(stderr, "BAUDPROBE-STDERR-MARKER-%d\n", getpid());
    fflush(stderr);
    os_log_t lg = os_log_create("dev.baud.probe", "probe");
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "BAUDPROBE-OSLOG-DEFAULT %{public}s %d", "public-str", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_INFO, "BAUDPROBE-OSLOG-INFO %{public}s %d", "public-str", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_DEBUG, "BAUDPROBE-OSLOG-DEBUG %{public}s %d", "public-str", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_ERROR, "BAUDPROBE-OSLOG-ERROR %{public}s %d", "public-str", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "BAUDPROBE-OSLOG-PRIVATE %s", "private-str");
    fprintf(out, "info_enabled=%d debug_enabled=%d\n", os_log_type_enabled(lg, OS_LOG_TYPE_INFO), os_log_type_enabled(lg, OS_LOG_TYPE_DEBUG));
    fclose(out);
    return 0;
}
