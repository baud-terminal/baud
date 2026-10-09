// Probe for note-draft "title": what a terminal can derive about its child.
// 1. tcgetpgrp() on the pty master, from a process that is NOT the slave's
//    session leader (as baud-app is), with an interactive shell and a job.
// 2. The cost of tcgetpgrp, proc_name, proc_pidpath, proc_pidinfo
//    (PROC_PIDVNODEPATHINFO), in ns per call, several rounds.
// 3. What each returns against a root-owned process (pid 1), the sudo case.
#include <errno.h>
#include <libproc.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#include <util.h>
#include <signal.h>
#include <fcntl.h>
#include <sys/sysctl.h>

static uint64_t now(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }

static void drain(int fd) {
    char buf[4096];
    for (;;) {
        ssize_t n = read(fd, buf, sizeof buf);
        if (n <= 0) break;
    }
}

static void say(int fd, const char *s) {
    if (write(fd, s, strlen(s)) < 0) perror("write");
}

static pid_t first_child(pid_t parent) {
    pid_t kids[64];
    int n = proc_listchildpids(parent, kids, sizeof kids);
    // proc_listchildpids returns a count of pids on current macOS; guard both readings.
    if (n <= 0) return -1;
    return kids[0];
}

static void cwd_of(pid_t pid, const char *label) {
    struct proc_vnodepathinfo vpi;
    int r = proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &vpi, sizeof vpi);
    if (r == (int)sizeof vpi)
        printf("  %s: cwd of %d = \"%s\" (ret %d = sizeof)\n", label, pid, vpi.pvi_cdir.vip_path, r);
    else
        printf("  %s: cwd of %d failed: ret %d errno %d (%s)\n", label, pid, r, errno, strerror(errno));
}

#define N 20000
#define ROUNDS 5

int main(int argc, char **argv) {
    const char *jobcmd = argc > 1 ? argv[1] : "/bin/sleep 5";
    int master;
    pid_t shell = forkpty(&master, NULL, NULL, NULL);
    if (shell < 0) { perror("forkpty"); return 1; }
    if (shell == 0) {
        setenv("PS1", "$ ", 1);
        execl("/bin/zsh", "zsh", "-f", "-i", (char *)NULL);
        _exit(127);
    }
    fcntl(master, F_SETFL, O_NONBLOCK);
    usleep(300000);
    drain(master);

    printf("sizeof(struct proc_vnodepathinfo) = %zu\n", sizeof(struct proc_vnodepathinfo));
    printf("PROC_PIDPATHINFO_MAXSIZE = %d, MAXCOMLEN = %d\n", PROC_PIDPATHINFO_MAXSIZE, MAXCOMLEN);
    printf("our pid %d, our sid %d; shell pid %d, shell sid %d, shell pgid %d\n",
           getpid(), getsid(0), shell, getsid(shell), getpgid(shell));

    // 1a. At the prompt: foreground group should be the shell's.
    errno = 0;
    pid_t fg = tcgetpgrp(master);
    printf("1a at prompt: tcgetpgrp(master) = %d errno %d (%s); shell pgid %d -> %s\n",
           fg, errno, strerror(errno), getpgid(shell), fg == getpgid(shell) ? "MATCH" : "DIFFER");
    cwd_of(shell, "1a");

    // 1b. A job in the foreground.
    { char line[256]; snprintf(line, sizeof line, "cd /tmp && %s\n", jobcmd); say(master, line); printf("  job command: %s", line); }
    // Wait (in this probe only) until the job owns the terminal, at most 3 s.
    for (int w = 0; w < 300 && tcgetpgrp(master) == getpgid(shell); w++) usleep(10000);
    drain(master);
    pid_t job = first_child(shell);
    { char cmd[256]; snprintf(cmd, sizeof cmd, "ps -o pid,ppid,pgid,tpgid,stat,comm -t %s", ptsname(master) + 5); printf("  ps: %s\n", cmd); fflush(stdout); system(cmd); }
    { pid_t kids[64]; int n = proc_listchildpids(shell, kids, sizeof kids); printf("  proc_listchildpids(shell) = %d\n", n); }
    errno = 0;
    fg = tcgetpgrp(master);
    char name[2 * MAXCOMLEN + 1] = {0};
    char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    int rn = proc_name(fg, name, sizeof name);
    int rp = proc_pidpath(fg, path, sizeof path);
    printf("1b job running: tcgetpgrp(master) = %d errno %d; job pid %d pgid %d -> %s\n",
           fg, errno, job, job > 0 ? getpgid(job) : -1,
           (job > 0 && fg == getpgid(job)) ? "MATCH job" : (fg == getpgid(shell) ? "still shell" : "DIFFER"));
    printf("  proc_name(%d) = \"%s\" (ret %d); proc_pidpath = \"%s\" (ret %d)\n", fg, name, rn, path, rp);
    cwd_of(fg, "1b job");
    cwd_of(shell, "1b shell");

    {
        int mib[3] = {CTL_KERN, KERN_PROCARGS2, fg};
        static char args[1 << 18]; size_t len = sizeof args;
        int r = sysctl(mib, 3, args, &len, NULL, 0);
        if (r == 0 && len > sizeof(int)) {
            char *exe = args + sizeof(int);           // exec path
            char *p = exe + strlen(exe);
            while (p < args + len && *p == 0) p++;    // skip padding to argv[0]
            printf("  KERN_PROCARGS2(%d): exec path \"%s\", argv[0] \"%s\", %zu bytes\n", fg, exe, p, len);
        } else printf("  KERN_PROCARGS2(%d) failed: %d errno %d\n", fg, r, errno);
    }
    // 2. Costs, against the foreground job, our own pid's master.
    printf("2 costs, ns per call, %d calls x %d rounds (min..max of rounds):\n", N, ROUNDS);
    const char *names[] = {"tcgetpgrp(master)", "proc_name", "proc_pidpath", "proc_pidinfo(VNODEPATHINFO)", "proc_pidinfo(TBSDINFO)", "sysctl(KERN_PROCARGS2)"};
    for (int k = 0; k < 6; k++) {
        double lo = 1e18, hi = 0;
        for (int r = 0; r < ROUNDS; r++) {
            uint64_t t0 = now();
            for (int i = 0; i < N; i++) {
                switch (k) {
                case 0: (void)tcgetpgrp(master); break;
                case 1: (void)proc_name(fg, name, sizeof name); break;
                case 2: (void)proc_pidpath(fg, path, sizeof path); break;
                case 3: { struct proc_vnodepathinfo v; (void)proc_pidinfo(fg, PROC_PIDVNODEPATHINFO, 0, &v, sizeof v); break; }
                case 4: { struct proc_bsdinfo b; (void)proc_pidinfo(fg, PROC_PIDTBSDINFO, 0, &b, sizeof b); break; }
                case 5: { int mib[3] = {CTL_KERN, KERN_PROCARGS2, fg}; static char a[1 << 18]; size_t l = sizeof a; (void)sysctl(mib, 3, a, &l, NULL, 0); break; }
                }
            }
            double per = (double)(now() - t0) / N;
            if (r == 0) continue; // first round dropped
            if (per < lo) lo = per;
            if (per > hi) hi = per;
        }
        printf("  %-30s %8.0f .. %8.0f\n", names[k], lo, hi);
    }

    // 3. A root-owned process.
    printf("3 root-owned pid 1:\n");
    memset(name, 0, sizeof name); memset(path, 0, sizeof path);
    errno = 0; rn = proc_name(1, name, sizeof name);
    printf("  proc_name(1) = \"%s\" ret %d errno %d (%s)\n", name, rn, errno, strerror(errno));
    errno = 0; rp = proc_pidpath(1, path, sizeof path);
    printf("  proc_pidpath(1) = \"%s\" ret %d errno %d (%s)\n", path, rp, errno, strerror(errno));
    errno = 0;
    cwd_of(1, "3");
    { int mib[3] = {CTL_KERN, KERN_PROCARGS2, 1}; static char a[1 << 18]; size_t l = sizeof a; errno = 0; int r = sysctl(mib, 3, a, &l, NULL, 0); printf("  KERN_PROCARGS2(1) ret %d errno %d (%s)\n", r, errno, strerror(errno)); }

    // 4. After the job: back to the shell.
    usleep(5000000);
    drain(master);
    errno = 0;
    fg = tcgetpgrp(master);
    printf("4 after job: tcgetpgrp(master) = %d; shell pgid %d -> %s\n", fg, getpgid(shell),
           fg == getpgid(shell) ? "MATCH shell" : "DIFFER");
    cwd_of(shell, "4");

    kill(shell, SIGHUP);
    close(master);
    int st; waitpid(shell, &st, 0);
    return 0;
}
