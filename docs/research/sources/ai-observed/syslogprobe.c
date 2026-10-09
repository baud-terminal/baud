#include <syslog.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
extern void syslog_legacy(int, const char *, ...) __asm("_syslog");
int main(void) {
    char *heap = malloc(64); snprintf(heap, 64, "heap-%d", getpid());
    syslog(LOG_WARNING, "SYSLOGPROBE-EXTSN-WARNING %s", heap);
    syslog(LOG_ERR, "SYSLOGPROBE-EXTSN-ERR %s", heap);
    syslog(LOG_INFO, "SYSLOGPROBE-EXTSN-INFO %s", heap);
    syslog_legacy(LOG_WARNING, "SYSLOGPROBE-LEGACY-WARNING %s", heap);
    printf("pid %d\n", getpid());
    return 0;
}
