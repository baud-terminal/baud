#include <os/log.h>
#include <mach/mach_time.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
static int cmp(const void *a, const void *b) { unsigned long long x=*(const unsigned long long*)a, y=*(const unsigned long long*)b; return x<y?-1:x>y; }
static void report(const char *name, unsigned long long *t, int n, mach_timebase_info_data_t tb) {
    qsort(t, n, sizeof *t, cmp);
    #define NS(v) ((double)(v) * tb.numer / tb.denom)
    printf("%-28s n=%d  min %.0f ns  median %.0f ns  p99 %.0f ns  max %.0f ns\n", name, n, NS(t[0]), NS(t[n/2]), NS(t[(n*99)/100]), NS(t[n-1]));
}
int main(void) {
    enum { N = 1000 };
    static unsigned long long t[N];
    mach_timebase_info_data_t tb; mach_timebase_info(&tb);
    os_log_t lg = os_log_create("dev.baud.probe3", "cost");
    char msg[200];
    snprintf(msg, sizeof msg, "images: a Sixel refused for its body, the last 342016 bytes (334 KiB) against a 262144-byte (256 KiB) body scratch; [images] body_kb raises it");
    for (int i = 0; i < N; i++) { unsigned long long a = mach_absolute_time(); os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "%{public}s", msg); t[i] = mach_absolute_time() - a; }
    report("os_log DEFAULT %{public}s", t, N, tb);
    for (int i = 0; i < N; i++) { unsigned long long a = mach_absolute_time(); os_log_with_type(lg, OS_LOG_TYPE_ERROR, "%{public}s", msg); t[i] = mach_absolute_time() - a; }
    report("os_log ERROR %{public}s", t, N, tb);
    for (int i = 0; i < N; i++) { unsigned long long a = mach_absolute_time(); os_log_with_type(lg, OS_LOG_TYPE_DEBUG, "%{public}s", msg); t[i] = mach_absolute_time() - a; }
    report("os_log DEBUG (disabled)", t, N, tb);
    int fd = open("costprobe-file.log", O_WRONLY|O_CREAT|O_APPEND|O_CLOEXEC, 0600);
    size_t len = strlen(msg); msg[len] = '\n';
    for (int i = 0; i < N; i++) { unsigned long long a = mach_absolute_time(); write(fd, msg, len+1); t[i] = mach_absolute_time() - a; }
    report("write(2) O_APPEND regular file", t, N, tb);
    close(fd);
    int dn = open("/dev/null", O_WRONLY);
    for (int i = 0; i < N; i++) { unsigned long long a = mach_absolute_time(); write(dn, msg, len+1); t[i] = mach_absolute_time() - a; }
    report("write(2) /dev/null", t, N, tb);
    return 0;
}
