// leds_probe.c -- what each candidate LED signal costs to read on macOS, and
// whether it means what its header says. Research probe, not baud code.
//
// Build: clang -O2 -Wall -o leds_probe leds_probe.c -lutil
// Run:   ./leds_probe <scratch-dir>
//
// Measures, on this Mac:
//   1. the foreground job of a pty, read from the MASTER (tcgetpgrp and
//      proc_pidinfo PROC_PIDTBSDINFO e_tpgid), after a child moves its own
//      process group to the foreground the way a shell's job control does;
//   2. the cost per call of every read a sampled LED would make;
//   3. the unit of rusage_info's ri_user_time (the header does not state it);
//   4. whether ri_diskio_bytesread/written count the storage layer only
//      (uncached file I/O) and not a pipe;
//   5. ri_phys_footprint's response to touching memory;
//   6. per-socket byte counts through the sysctl net.inet.tcp.pcblist_n, whose
//      record layout is xnu's PRIVATE (not in the SDK): struct copied from
//      xnu-12377.121.6 bsd/sys/socketvar.h.

#include <errno.h>
#include <fcntl.h>
#include <libproc.h>
#include <mach/mach_time.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/sysctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <util.h>

static mach_timebase_info_data_t tb;
static uint64_t ns(uint64_t t) { return t * tb.numer / tb.denom; }

static int cmp_u64(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return x < y ? -1 : x > y;
}

#define ROUNDS 7
#define N 20000
// Times `body` N times per round; prints min and median ns per call.
#define TIMEN(label, cnt, body)                                              \
    do {                                                                     \
        uint64_t r[ROUNDS];                                                  \
        for (int k = 0; k < ROUNDS; k++) {                                   \
            uint64_t t0 = mach_absolute_time();                              \
            for (int i = 0; i < (cnt); i++) { body; }                        \
            r[k] = ns(mach_absolute_time() - t0);                            \
        }                                                                    \
        qsort(r, ROUNDS, sizeof r[0], cmp_u64);                              \
        printf("  %-44s min %9.0f  median %9.0f ns/call\n", label,           \
               (double)r[0] / (cnt), (double)r[ROUNDS / 2] / (cnt));         \
    } while (0)
#define TIME(label, body) TIMEN(label, N, body)

// ---- xnu PRIVATE layouts, copied from bsd/sys/socketvar.h (xnu-12377.121.6)
#pragma pack(4) /* socketvar.h line 425: the PRIVATE block is packed to 4 */
#define XSO_SOCKET 0x001
#define XSO_STATS 0x008
#define SO_TC_STATS_MAX 4
struct data_stats { uint64_t rxpackets, rxbytes, txpackets, txbytes; };
struct xsocket_n {
    uint32_t xso_len; uint32_t xso_kind; uint64_t xso_so; short so_type;
    uint32_t so_options; short so_linger; short so_state; uint64_t so_pcb;
    int xso_protocol; int xso_family; short so_qlen; short so_incqlen;
    short so_qlimit; short so_timeo; unsigned short so_error; pid_t so_pgid;
    uint32_t so_oobmark; uid_t so_uid; pid_t so_last_pid; pid_t so_e_pid;
    uint64_t so_gencnt; uint32_t so_flags; uint32_t so_flags1;
    int32_t so_usecount; int32_t so_retaincnt; uint32_t xso_filter_flags;
};
struct xsockstat_n {
    uint32_t xst_len; uint32_t xst_kind;
    struct data_stats xst_tc_stats[SO_TC_STATS_MAX];
};
#pragma pack()
#define ROUNDUP64(x) (((x) + 7) & ~(size_t)7)

static char *pcb_buf;
static size_t pcb_cap;
static size_t pcb_sized; // what the size query (NULL buffer) said

// Fetches net.inet.tcp.pcblist_n; returns bytes copied out, or 0.
static size_t pcblist_fetch(void) {
    size_t len = 0;
    if (sysctlbyname("net.inet.tcp.pcblist_n", NULL, &len, NULL, 0) != 0) return 0;
    pcb_sized = len;
    if (len > pcb_cap) { pcb_cap = len * 2; pcb_buf = realloc(pcb_buf, pcb_cap); }
    len = pcb_cap;
    if (sysctlbyname("net.inet.tcp.pcblist_n", pcb_buf, &len, NULL, 0) != 0) return 0;
    return len;
}

// Sums data bytes of every socket whose so_last_pid is `pid`.
static int pcblist_sum(size_t len, pid_t pid, uint64_t *rx, uint64_t *tx) {
    *rx = *tx = 0;
    int nsock = 0, matched = 0, mine = 0;
    size_t off = ROUNDUP64(*(uint32_t *)pcb_buf); // skip the leading xinpgen
    while (off + 8 <= len) {
        uint32_t ilen = *(uint32_t *)(pcb_buf + off);
        uint32_t kind = *(uint32_t *)(pcb_buf + off + 4);
        if (ilen == 0) break;
        if (kind == XSO_SOCKET) {
            struct xsocket_n *x = (struct xsocket_n *)(pcb_buf + off);
            nsock++;
            mine = (x->so_last_pid == pid);
            if (getenv("PCB_DEBUG"))
                printf("    xsocket_n len %u (sizeof %zu) family %d proto %d last_pid %d e_pid %d uid %u\n",
                       x->xso_len, sizeof *x, x->xso_family, x->xso_protocol, x->so_last_pid, x->so_e_pid, x->so_uid);
            matched += mine;
        } else if (kind == XSO_STATS) {
            struct xsockstat_n *s = (struct xsockstat_n *)(pcb_buf + off);
            if (getenv("PCB_DEBUG"))
                printf("    xsockstat_n len %u (sizeof %zu) rx %llu tx %llu\n", s->xst_len, sizeof *s,
                       s->xst_tc_stats[0].rxbytes, s->xst_tc_stats[0].txbytes);
            if (mine) {
                *rx += s->xst_tc_stats[0].rxbytes;
                *tx += s->xst_tc_stats[0].txbytes;
            }
            mine = 0;
        }
        off += ROUNDUP64(ilen);
    }
    printf("    sockets in dump: %d (header xig_count %u), with so_last_pid == %d: %d\n", nsock,
           len >= 8 ? *(uint32_t *)(pcb_buf + 4) : 0, pid, matched);
    return matched;
}

static void busy(double seconds) {
    uint64_t end = mach_absolute_time() + (uint64_t)(seconds * 1e9 * tb.denom / tb.numer);
    volatile uint64_t x = 0;
    while (mach_absolute_time() < end) x++;
}

static struct rusage_info_v4 ru(pid_t pid) {
    struct rusage_info_v4 r;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&r) != 0)
        perror("proc_pid_rusage");
    return r;
}


// 7-9: whose counters move, an unreadable process, and a memory *event*.
static void extra(const char *dir) {
    // ---- 7. a child's disk I/O and the parent's counters ------------------
    printf("\n7. a child's uncached I/O: the child's counters vs the parent's, before and after waitpid\n");
    int go[2], done[2];
    pipe(go); pipe(done);
    pid_t parent = getpid();
    struct rusage_info_v4 p0 = ru(parent);
    pid_t kid = fork();
    if (kid == 0) {
        char c;
        const size_t SZ = 16u << 20;
        char *b = malloc(SZ);
        memset(b, 1, SZ);
        char fp[1024];
        snprintf(fp, sizeof fp, "%s/leds_probe_child.bin", dir);
        int fd = open(fp, O_CREAT | O_TRUNC | O_RDWR, 0600);
        fcntl(fd, F_NOCACHE, 1);
        if (write(fd, b, SZ) != (ssize_t)SZ) _exit(2);
        fsync(fd);
        close(fd);
        unlink(fp);
        write(done[1], "x", 1);   // I/O finished; still alive
        read(go[0], &c, 1);       // wait until the parent has looked
        _exit(0);
    }
    char c;
    read(done[0], &c, 1);
    struct rusage_info_v4 k = ru(kid);
    struct rusage_info_v4 p1 = ru(parent);
    printf("  child alive, after writing 16 MiB uncached: child written %llu; parent written +%llu, child_* fields n/a in diskio\n",
           k.ri_diskio_byteswritten, p1.ri_diskio_byteswritten - p0.ri_diskio_byteswritten);
    write(go[1], "x", 1);
    int st;
    waitpid(kid, &st, 0);
    struct rusage_info_v4 p2 = ru(parent);
    printf("  after waitpid: parent written +%llu (from before the fork)\n",
           p2.ri_diskio_byteswritten - p0.ri_diskio_byteswritten);

    // ---- 8. a process this user cannot read --------------------------------
    printf("\n8. reading a process owned by root (pid 1)\n");
    struct rusage_info_v4 r;
    errno = 0;
    int rc = proc_pid_rusage(1, RUSAGE_INFO_V4, (rusage_info_t *)&r);
    printf("  proc_pid_rusage(1) = %d, errno %d (%s)\n", rc, errno, rc ? strerror(errno) : "-");
    char nm[64] = {0};
    errno = 0;
    rc = proc_name(1, nm, sizeof nm);
    printf("  proc_name(1) = %d '%s' errno %d\n", rc, nm, errno);
    struct proc_bsdshortinfo si;
    errno = 0;
    rc = proc_pidinfo(1, PROC_PIDT_SHORTBSDINFO, 0, &si, sizeof si);
    printf("  proc_pidinfo(1, PROC_PIDT_SHORTBSDINFO) = %d errno %d pbsi_comm '%s'\n", rc, errno, rc > 0 ? si.pbsi_comm : "");
    struct proc_fdinfo fds[16];
    errno = 0;
    rc = proc_pidinfo(1, PROC_PIDLISTFDS, 0, fds, sizeof fds);
    printf("  proc_pidinfo(1, PROC_PIDLISTFDS) = %d errno %d\n", rc, errno);

    // ---- 9. ri_pageins: a memory event counter -----------------------------
    printf("\n9. ri_pageins on touching an uncached file mapping\n");
    const size_t SZ = 32u << 20;
    char fp[1024];
    snprintf(fp, sizeof fp, "%s/leds_probe_map.bin", dir);
    int fd = open(fp, O_CREAT | O_TRUNC | O_RDWR, 0600);
    fcntl(fd, F_NOCACHE, 1); // written past the cache, so the mapping must page in
    char *b = malloc(1 << 20);
    memset(b, 3, 1 << 20);
    for (int i = 0; i < 32; i++) write(fd, b, 1 << 20);
    fsync(fd);
    close(fd);
    // Purge this file from the cache: reopen with F_NOCACHE and drop pages.
    fd = open(fp, O_RDONLY);
    fcntl(fd, F_NOCACHE, 1);
    struct rusage_info_v4 m0 = ru(getpid());
    volatile char *map = mmap(NULL, SZ, PROT_READ, MAP_PRIVATE, fd, 0);
    if (map == MAP_FAILED) { perror("mmap"); return; }
    unsigned sum = 0;
    for (size_t i = 0; i < SZ; i += 16384) sum += (unsigned char)map[i];
    struct rusage_info_v4 m1 = ru(getpid());
    printf("  touched %zu B of a fresh mapping (written with F_NOCACHE): ri_pageins +%llu (sum %u)\n",
           SZ, m1.ri_pageins - m0.ri_pageins, sum);
    munmap((void *)map, SZ);
    close(fd);
    unlink(fp);
}

int main(int argc, char **argv) {
    mach_timebase_info(&tb);
    printf("mach_timebase_info: numer %u denom %u\n", tb.numer, tb.denom);
    if (argc > 2 && strcmp(argv[1], "--pid") == 0) {
        // Live mode: sum the TCP data bytes of every socket last used by an
        // existing process, twice, one second apart; and read its rusage.
        pid_t pid = (pid_t)atoi(argv[2]);
        uint64_t rx0, tx0, rx1, tx1;
        size_t len = pcblist_fetch();
        printf("pcblist_n size query %zu B, fetch %zu B\n", pcb_sized, len);
        pcblist_sum(len, pid, &rx0, &tx0);
        struct rusage_info_v4 a = ru(pid);
        sleep(1);
        len = pcblist_fetch();
        pcblist_sum(len, pid, &rx1, &tx1);
        struct rusage_info_v4 b = ru(pid);
        printf("pid %d sockets: rx %llu -> %llu (+%llu), tx %llu -> %llu (+%llu) over 1 s\n", pid,
               rx0, rx1, rx1 - rx0, tx0, tx1, tx1 - tx0);
        // Public path: PROC_PIDLISTFDS, then PROC_PIDFDSOCKETINFO per socket.
        struct proc_fdinfo fds[256];
        int nfd = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, sizeof fds);
        printf("PROC_PIDLISTFDS = %d bytes (%d fds)\n", nfd, nfd > 0 ? nfd / (int)sizeof fds[0] : 0);
        for (int i = 0; nfd > 0 && i < nfd / (int)sizeof fds[0]; i++) {
            if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) continue;
            struct socket_fdinfo si;
            int got = proc_pidfdinfo(pid, fds[i].proc_fd, PROC_PIDFDSOCKETINFO, &si, sizeof si);
            if (got <= 0) { printf("  fd %d: proc_pidfdinfo failed errno %d\n", fds[i].proc_fd, errno); continue; }
            printf("  fd %d: family %d kind %d tcp state %d rcv sbi_cc %u snd sbi_cc %u (bytes queued now)\n",
                   fds[i].proc_fd, si.psi.soi_family, si.psi.soi_kind,
                   si.psi.soi_kind == SOCKINFO_TCP ? si.psi.soi_proto.pri_tcp.tcpsi_state : -1,
                   si.psi.soi_rcv.sbi_cc, si.psi.soi_snd.sbi_cc);
            volatile int sink = 0;
            const int sfd = fds[i].proc_fd; // TIMEN's loop shadows i
            TIMEN("proc_pidfdinfo(PROC_PIDFDSOCKETINFO)", 20000,
                  sink += proc_pidfdinfo(pid, sfd, PROC_PIDFDSOCKETINFO, &si, sizeof si));
            TIMEN("proc_pidinfo(PROC_PIDLISTFDS)", 20000,
                  sink += proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, sizeof fds));
            break;
        }
        printf("pid %d rusage: user +%llu ticks, sys +%llu ticks, diskio r +%llu w +%llu, footprint %llu\n",
               pid, b.ri_user_time - a.ri_user_time, b.ri_system_time - a.ri_system_time,
               b.ri_diskio_bytesread - a.ri_diskio_bytesread,
               b.ri_diskio_byteswritten - a.ri_diskio_byteswritten, b.ri_phys_footprint);
        return 0;
    }
    const char *dir = argc > 1 ? argv[1] : ".";

    // ---- 1. foreground job, seen from the master --------------------------
    int master, slave;
    if (openpty(&master, &slave, NULL, NULL, NULL) != 0) { perror("openpty"); return 1; }
    pid_t shell = fork();
    if (shell == 0) {
        // The "shell": session leader with the slave as controlling tty.
        close(master);
        setsid();
        ioctl(slave, TIOCSCTTY, 0);
        pid_t job = fork();
        if (job == 0) {
            // The "job": its own process group, moved to the foreground, the
            // way a job-control shell does before exec.
            setpgid(0, 0);
            signal(SIGTTOU, SIG_IGN);
            tcsetpgrp(slave, getpgrp());
            execl("/bin/sleep", "sleep", "3", (char *)NULL);
            _exit(127);
        }
        int st; waitpid(job, &st, 0);
        _exit(0);
    }
    close(slave);
    usleep(300 * 1000);
    pid_t fg = tcgetpgrp(master);
    printf("\n1. foreground from the master\n");
    printf("  shell pid %d; tcgetpgrp(master) = %d (errno %d)\n", shell, fg, fg < 0 ? errno : 0);
    struct proc_bsdinfo bi;
    int n = proc_pidinfo(shell, PROC_PIDTBSDINFO, 0, &bi, sizeof bi);
    printf("  proc_pidinfo(shell, PROC_PIDTBSDINFO) = %d bytes; e_tpgid %u pbi_pgid %u comm '%s'\n",
           n, bi.e_tpgid, bi.pbi_pgid, bi.pbi_comm);
    char name[2 * MAXCOMLEN + 1] = {0}, path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    proc_name(fg, name, sizeof name);
    proc_pidpath(fg, path, sizeof path);
    printf("  proc_name(fg) '%s'  proc_pidpath(fg) '%s'\n", name, path);
    pid_t members[64];
    int nb = proc_listpgrppids(fg, members, sizeof members);
    printf("  proc_listpgrppids(fg) = %d (libproc.c returns a count of pids)\n", nb);

    // ---- 2. cost per call --------------------------------------------------
    printf("\n2. cost per call (N=%d, %d rounds)\n", N, ROUNDS);
    volatile int sink = 0;
    TIME("tcgetpgrp(master)", sink += tcgetpgrp(master));
    TIME("proc_pidinfo(PROC_PIDTBSDINFO)", sink += proc_pidinfo(shell, PROC_PIDTBSDINFO, 0, &bi, sizeof bi));
    struct proc_bsdshortinfo si;
    TIME("proc_pidinfo(PROC_PIDT_SHORTBSDINFO)", sink += proc_pidinfo(fg, PROC_PIDT_SHORTBSDINFO, 0, &si, sizeof si));
    TIME("proc_name(fg)", sink += proc_name(fg, name, sizeof name));
    TIME("proc_pidpath(fg)", sink += proc_pidpath(fg, path, sizeof path));
    TIME("proc_listpgrppids(fg)", sink += proc_listpgrppids(fg, members, sizeof members));
    struct rusage_info_v4 r4;
    TIME("proc_pid_rusage(fg, RUSAGE_INFO_V4)", sink += proc_pid_rusage(fg, RUSAGE_INFO_V4, (rusage_info_t *)&r4));
    TIME("getpid() (a syscall floor for comparison)", sink += (int)syscall(20));

    // ---- 3. unit of ri_user_time ------------------------------------------
    printf("\n3. ri_user_time unit\n");
    struct rusage g0, g1;
    struct rusage_info_v4 a = ru(getpid());
    getrusage(RUSAGE_SELF, &g0);
    busy(0.5);
    struct rusage_info_v4 b = ru(getpid());
    getrusage(RUSAGE_SELF, &g1);
    double gr_ns = ((g1.ru_utime.tv_sec - g0.ru_utime.tv_sec) * 1e9 +
                    (g1.ru_utime.tv_usec - g0.ru_utime.tv_usec) * 1e3);
    double ri = (double)(b.ri_user_time - a.ri_user_time);
    printf("  getrusage user delta %.0f ns; ri_user_time delta %.0f; ratio getrusage/ri %.3f\n",
           gr_ns, ri, gr_ns / ri);
    printf("  (numer/denom = %.3f: if the ratio matches it, ri_user_time is mach ticks)\n",
           (double)tb.numer / tb.denom);

    // ---- 4. ri_diskio: storage layer, not a pipe ---------------------------
    printf("\n4. ri_diskio_bytesread/written\n");
    const size_t SZ = 32u << 20;
    char *buf = malloc(SZ);
    memset(buf, 0x5a, SZ);
    char fpath[1024];
    snprintf(fpath, sizeof fpath, "%s/leds_probe.bin", dir);
    a = ru(getpid());
    int fd = open(fpath, O_CREAT | O_TRUNC | O_RDWR, 0600);
    fcntl(fd, F_NOCACHE, 1);
    if (write(fd, buf, SZ) != (ssize_t)SZ) perror("write");
    fsync(fd);
    b = ru(getpid());
    printf("  uncached write %zu B + fsync: diskio written +%llu, read +%llu\n", SZ,
           b.ri_diskio_byteswritten - a.ri_diskio_byteswritten, b.ri_diskio_bytesread - a.ri_diskio_bytesread);
    a = b;
    lseek(fd, 0, SEEK_SET);
    if (read(fd, buf, SZ) != (ssize_t)SZ) perror("read");
    b = ru(getpid());
    printf("  uncached read  %zu B:         diskio read +%llu, written +%llu\n", SZ,
           b.ri_diskio_bytesread - a.ri_diskio_bytesread, b.ri_diskio_byteswritten - a.ri_diskio_byteswritten);
    a = b;
    fcntl(fd, F_NOCACHE, 0);
    lseek(fd, 0, SEEK_SET);
    if (read(fd, buf, SZ) != (ssize_t)SZ) perror("read2");
    lseek(fd, 0, SEEK_SET);
    if (read(fd, buf, SZ) != (ssize_t)SZ) perror("read3");
    b = ru(getpid());
    printf("  cached read x2 %zu B:         diskio read +%llu\n", SZ, b.ri_diskio_bytesread - a.ri_diskio_bytesread);
    close(fd);
    unlink(fpath);
    int p[2];
    pipe(p);
    a = ru(getpid());
    size_t moved = 0;
    while (moved < SZ) {
        ssize_t w = write(p[1], buf, 16384);
        ssize_t rr = read(p[0], buf, 16384);
        if (w <= 0 || rr <= 0) break;
        moved += (size_t)rr;
    }
    b = ru(getpid());
    printf("  pipe %zu B through:           diskio read +%llu written +%llu\n", moved,
           b.ri_diskio_bytesread - a.ri_diskio_bytesread, b.ri_diskio_byteswritten - a.ri_diskio_byteswritten);

    // ---- 5. phys_footprint -------------------------------------------------
    printf("\n5. ri_phys_footprint\n");
    a = ru(getpid());
    const size_t M = 64u << 20;
    volatile char *mem = malloc(M);
    for (size_t i = 0; i < M; i += 4096) mem[i] = 1;
    b = ru(getpid());
    printf("  touch %zu B: phys_footprint %llu -> %llu (+%lld), resident +%lld\n", M,
           a.ri_phys_footprint, b.ri_phys_footprint,
           (long long)(b.ri_phys_footprint - a.ri_phys_footprint),
           (long long)(b.ri_resident_size - a.ri_resident_size));
    free((void *)mem);

    // ---- 6. per-socket bytes via net.inet.tcp.pcblist_n --------------------
    printf("\n6. net.inet.tcp.pcblist_n (xnu PRIVATE layout)\n");
    int ls = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in sa = {.sin_len = sizeof sa, .sin_family = AF_INET,
                             .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    bind(ls, (struct sockaddr *)&sa, sizeof sa);
    socklen_t sl = sizeof sa;
    getsockname(ls, (struct sockaddr *)&sa, &sl);
    listen(ls, 1);
    int cs = socket(AF_INET, SOCK_STREAM, 0);
    connect(cs, (struct sockaddr *)&sa, sizeof sa);
    int as = accept(ls, NULL, NULL);
    uint64_t rx0, tx0, rx1, tx1;
    size_t len = pcblist_fetch();
    pcblist_sum(len, getpid(), &rx0, &tx0);
    const size_t NET = 1u << 20;
    size_t sent = 0, got = 0;
    while (sent < NET) {
        ssize_t w = send(cs, buf, 65536, 0);
        if (w <= 0) break;
        sent += (size_t)w;
        while (got < sent) {
            ssize_t rr = recv(as, buf, 65536, 0);
            if (rr <= 0) break;
            got += (size_t)rr;
        }
    }
    len = pcblist_fetch();
    pcblist_sum(len, getpid(), &rx1, &tx1);
    printf("  sent %zu B client->server over loopback; this pid's sockets: rx +%llu tx +%llu\n",
           sent, rx1 - rx0, tx1 - tx0);
    printf("  dump size %zu B\n", len);
    TIMEN("sysctl pcblist_n size query + fetch", 200, sink += (int)pcblist_fetch());

    extra(dir);

    kill(shell, SIGTERM);
    int st; waitpid(shell, &st, 0);
    return sink == 42 ? 1 : 0;
}
