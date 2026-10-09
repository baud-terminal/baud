// child_probe.c -- whether rusage_info's ri_child_* fields fold in a child's
// CPU and page-ins, and when: while it runs, or only after waitpid. The
// question is whether a sampler that reads the shell (or a group member)
// sees a short-lived child that was started and reaped between two samples.
// Build: clang -O2 -o child_probe child_probe.c
#include <fcntl.h>
#include <libproc.h>
#include <mach/mach_time.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

static struct rusage_info_v4 ru(pid_t pid) {
    struct rusage_info_v4 r;
    memset(&r, 0, sizeof r);
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&r) != 0) perror("proc_pid_rusage");
    return r;
}

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : ".";
    char fp[1024];
    snprintf(fp, sizeof fp, "%s/child_probe.bin", dir);
    // A 16 MiB file written past the cache, for the child to page in.
    int fd = open(fp, O_CREAT | O_TRUNC | O_RDWR, 0600);
    fcntl(fd, F_NOCACHE, 1);
    char *b = malloc(1 << 20);
    memset(b, 7, 1 << 20);
    for (int i = 0; i < 16; i++) if (write(fd, b, 1 << 20) != 1 << 20) return 2;
    fsync(fd);
    close(fd);

    int done[2], go[2];
    if (pipe(done) || pipe(go)) return 2;
    struct rusage_info_v4 p0 = ru(getpid());
    pid_t kid = fork();
    if (kid == 0) {
        mach_timebase_info_data_t tb;
        mach_timebase_info(&tb);
        uint64_t end = mach_absolute_time() + (uint64_t)(0.3e9 * tb.denom / tb.numer);
        volatile uint64_t x = 0;
        while (mach_absolute_time() < end) x++;
        int f = open(fp, O_RDONLY);
        fcntl(f, F_NOCACHE, 1);
        volatile char *m = mmap(NULL, 16u << 20, PROT_READ, MAP_PRIVATE, f, 0);
        unsigned s = 0;
        for (size_t i = 0; i < (16u << 20); i += 16384) s += (unsigned char)m[i];
        char c = (char)s;
        write(done[1], &c, 1);
        read(go[0], &c, 1);
        _exit(0);
    }
    char c;
    read(done[0], &c, 1);
    struct rusage_info_v4 k = ru(kid);
    struct rusage_info_v4 p1 = ru(getpid());
    printf("child alive: child user %llu ticks, system %llu ticks, pageins %llu, diskio read %llu\n",
           k.ri_user_time, k.ri_system_time, k.ri_pageins, k.ri_diskio_bytesread);
    printf("  parent while child alive: child_user +%llu, child_system +%llu, child_pageins +%llu\n",
           p1.ri_child_user_time - p0.ri_child_user_time, p1.ri_child_system_time - p0.ri_child_system_time,
           p1.ri_child_pageins - p0.ri_child_pageins);
    write(go[1], "x", 1);
    int st;
    waitpid(kid, &st, 0);
    struct rusage_info_v4 p2 = ru(getpid());
    printf("  parent after waitpid:     child_user +%llu, child_system +%llu, child_pageins +%llu, own diskio read +%llu\n",
           p2.ri_child_user_time - p0.ri_child_user_time, p2.ri_child_system_time - p0.ri_child_system_time,
           p2.ri_child_pageins - p0.ri_child_pageins, p2.ri_diskio_bytesread - p0.ri_diskio_bytesread);
    unlink(fp);
    return 0;
}
