// pcb64_probe.c -- net.inet.tcp.pcblist64, whose record (struct xtcpcb64 in
// netinet/tcp_var.h) IS declared in the macOS SDK. Does it give a byte count
// for a TCP connection, can a record be tied to a process's socket, what does
// a fetch cost, and which sockets does the dump show? Research probe.
// Build: clang -O2 -o pcb64_probe pcb64_probe.c
// Run:   ./pcb64_probe            (own loopback pair)
//        ./pcb64_probe <pid>      (also: is that process's TCP socket in the dump?)
#include <arpa/inet.h>
#include <errno.h>
#include <libproc.h>
#include <mach/mach_time.h>
#include <netinet/in.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#include <sys/socketvar.h>
#include <sys/sysctl.h>
#include <unistd.h>
#include <netinet/in_systm.h>
#include <netinet/ip.h>
#include <netinet/in_pcb.h>
#include <netinet/tcp.h>
#include <netinet/tcp_timer.h>
#include <netinet/tcp_var.h>

static char *buf;
static size_t cap, sized;

static size_t fetch(void) {
    size_t len = 0;
    if (sysctlbyname("net.inet.tcp.pcblist64", NULL, &len, NULL, 0) != 0) { perror("size"); return 0; }
    sized = len;
    if (len > cap) { cap = len * 2; buf = realloc(buf, cap); }
    len = cap;
    if (sysctlbyname("net.inet.tcp.pcblist64", buf, &len, NULL, 0) != 0) { perror("fetch"); return 0; }
    return len;
}

// Finds the record whose local and foreign ports (host order) match.
static const struct xtcpcb64 *find(size_t len, int lport, int fport, int *nrec, unsigned *count) {
    const struct xinpgen *xig = (const struct xinpgen *)buf;
    *count = xig->xig_count;
    *nrec = 0;
    const struct xtcpcb64 *hit = NULL;
    size_t off = xig->xig_len;
    while (off + sizeof(uint32_t) <= len) {
        const struct xtcpcb64 *xt = (const struct xtcpcb64 *)(buf + off);
        if (xt->xt_len <= sizeof(struct xinpgen)) break; // the trailing xinpgen
        (*nrec)++;
        if (ntohs(xt->xt_inpcb.inp_lport) == lport && ntohs(xt->xt_inpcb.inp_fport) == fport) hit = xt;
        off += xt->xt_len;
    }
    return hit;
}

static int ports_of(pid_t pid, int fd, int *lport, int *fport) {
    struct socket_fdinfo si;
    if (proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, &si, sizeof si) <= 0) return -1;
    if (si.psi.soi_kind != SOCKINFO_TCP) return -1;
    // insi_lport/fport hold the port in network byte order (measured: the
    // raw values are the byte-swapped ports getsockname reports).
    *lport = ntohs((uint16_t)si.psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport);
    *fport = ntohs((uint16_t)si.psi.soi_proto.pri_tcp.tcpsi_ini.insi_fport);
    return 0;
}

int main(int argc, char **argv) {
    printf("sizeof(struct xtcpcb64) %zu, sizeof(struct xinpgen) %zu\n", sizeof(struct xtcpcb64), sizeof(struct xinpgen));
    // A loopback pair in this process.
    int ls = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in sa = {.sin_len = sizeof sa, .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    bind(ls, (struct sockaddr *)&sa, sizeof sa);
    socklen_t sl = sizeof sa;
    getsockname(ls, (struct sockaddr *)&sa, &sl);
    listen(ls, 1);
    int cs = socket(AF_INET, SOCK_STREAM, 0);
    connect(cs, (struct sockaddr *)&sa, sizeof sa);
    int as = accept(ls, NULL, NULL);

    // Attribution: this process's socket ports through PROC_PIDFDSOCKETINFO.
    int cl, cf, al, af;
    if (ports_of(getpid(), cs, &cl, &cf) || ports_of(getpid(), as, &al, &af)) { puts("ports_of failed"); return 1; }
    struct sockaddr_in me; socklen_t ml = sizeof me; getsockname(cs, (struct sockaddr *)&me, &ml);
    printf("client socket ports %d->%d, accepted %d->%d (PROC_PIDFDSOCKETINFO, ntohs); getsockname says client port %d, listener %d\n",
           cl, cf, al, af, ntohs(me.sin_port), ntohs(sa.sin_port));

    int n; unsigned cnt;
    size_t len = fetch();
    const struct xtcpcb64 *c0 = find(len, cl, cf, &n, &cnt);
    struct xtcpcb64 c0v = c0 ? *c0 : (struct xtcpcb64){0};
    const struct xtcpcb64 *a0 = find(len, al, af, &n, &cnt);
    struct xtcpcb64 a0v = a0 ? *a0 : (struct xtcpcb64){0};
    printf("before: size query %zu B, fetch %zu B, xig_count %u, records walked %d; client %s, accepted %s\n",
           sized, len, cnt, n, c0 ? "found" : "MISSING", a0 ? "found" : "MISSING");

    static char data[65536];
    const size_t NET = 1u << 20;
    size_t sent = 0, got = 0;
    while (sent < NET) {
        ssize_t w = send(cs, data, sizeof data, 0);
        if (w <= 0) break;
        sent += (size_t)w;
        while (got < sent) { ssize_t r = recv(as, data, sizeof data, 0); if (r <= 0) break; got += (size_t)r; }
    }
    len = fetch();
    const struct xtcpcb64 *c1 = find(len, cl, cf, &n, &cnt);
    const struct xtcpcb64 *a1 = find(len, al, af, &n, &cnt);
    if (!c0 || !a0 || !c1 || !a1) { puts("a record is missing"); return 1; }
    printf("after %zu B client->server: sender d(snd_una-iss) %u, d(snd_max-iss) %u; receiver d(rcv_nxt-irs) %u\n", sent,
           (uint32_t)((c1->snd_una - c1->iss) - (c0v.snd_una - c0v.iss)),
           (uint32_t)((c1->snd_max - c1->iss) - (c0v.snd_max - c0v.iss)),
           (uint32_t)((a1->rcv_nxt - a1->irs) - (a0v.rcv_nxt - a0v.irs)));
    printf("sender t_rxtshift %d, snd_nxt %s snd_max; t_state %d; so_uid %u so_pgid %d (no pid field in xsocket64)\n",
           c1->t_rxtshift, c1->snd_nxt < c1->snd_max ? "<" : ">=", c1->t_state,
           c1->xt_inpcb.xi_socket.so_uid, c1->xt_inpcb.xi_socket.so_pgid);

    // Cost of one size query plus fetch.
    mach_timebase_info_data_t tb; mach_timebase_info(&tb);
    uint64_t r[7];
    for (int k = 0; k < 7; k++) {
        uint64_t t0 = mach_absolute_time();
        for (int i = 0; i < 200; i++) fetch();
        r[k] = (mach_absolute_time() - t0) * tb.numer / tb.denom;
    }
    for (int i = 0; i < 7; i++) for (int j = i + 1; j < 7; j++) if (r[j] < r[i]) { uint64_t t = r[i]; r[i] = r[j]; r[j] = t; }
    printf("pcblist64 size query + fetch (dump %zu B): min %.0f median %.0f ns/call (200 calls x 7 rounds)\n",
           len, r[0] / 200.0, r[3] / 200.0);

    if (argc > 1) {
        pid_t pid = (pid_t)atoi(argv[1]);
        struct proc_fdinfo fds[256];
        int nb = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, sizeof fds);
        for (int i = 0; nb > 0 && i < nb / (int)sizeof fds[0]; i++) {
            if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) continue;
            int lp, fp;
            if (ports_of(pid, fds[i].proc_fd, &lp, &fp)) continue;
            len = fetch();
            const struct xtcpcb64 *h = find(len, lp, fp, &n, &cnt);
            printf("other process: a TCP socket (ports withheld here); dump size query %zu B, fetch %zu B, xig_count %u, records %d; its record %s\n",
                   sized, len, cnt, n, h ? "FOUND" : "absent");
        }
    }
    return 0;
}
