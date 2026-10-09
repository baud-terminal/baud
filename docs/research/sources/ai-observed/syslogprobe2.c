#include <syslog.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
extern void syslog_legacy(int, const char *, ...) __asm("_syslog");
int main(void) {
    char *heap = malloc(64); snprintf(heap, 64, "heap-%d", getpid());
    syslog(LOG_CRIT,  "SYSLOG2-EXTSN-CRIT %s", heap);
    syslog(LOG_ALERT, "SYSLOG2-EXTSN-ALERT %s", heap);
    syslog(LOG_ERR,   "SYSLOG2-EXTSN-ERR %s", heap);
    syslog(LOG_NOTICE,"SYSLOG2-EXTSN-NOTICE %s", heap);
    syslog(LOG_DEBUG, "SYSLOG2-EXTSN-DEBUG %s", heap);
    syslog_legacy(LOG_ERR,  "SYSLOG2-LEGACY-ERR %s", heap);
    syslog_legacy(LOG_INFO, "SYSLOG2-LEGACY-INFO %s", heap);
    syslog(LOG_WARNING, "SYSLOG2-PRIVATE-ANNOT %{private}s", heap);
    syslog(LOG_WARNING, "SYSLOG2-PUBLIC-ANNOT %{public}s", heap);
    syslog(LOG_WARNING, "SYSLOG2-CONTROL plain");
    printf("pid %d\n", getpid());
    return 0;
}
