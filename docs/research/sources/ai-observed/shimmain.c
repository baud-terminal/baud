#include <os/log.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
void baud_os_log(os_log_t log, os_log_type_t type, const char *message);
int main(void) {
    os_log_t lg = os_log_create("dev.baud.probe5", "shim");
    char *m = malloc(64); snprintf(m, 64, "SHIMPROBE heap-%d", getpid());
    baud_os_log(lg, OS_LOG_TYPE_DEFAULT, m);
    baud_os_log(lg, OS_LOG_TYPE_ERROR, m);
    printf("sizeof(os_log_type_t)=%zu ERROR=%d FAULT=%d\n", sizeof(os_log_type_t), OS_LOG_TYPE_ERROR, OS_LOG_TYPE_FAULT);
    return 0;
}
