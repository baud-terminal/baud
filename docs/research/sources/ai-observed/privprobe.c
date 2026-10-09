#include <os/log.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
int main(int argc, char **argv) {
    os_log_t lg = os_log_create("dev.baud.probe2", "priv");
    char *heap = malloc(64);
    snprintf(heap, 64, "heap-str-%d", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P2-DEFAULT-HEAP-PRIVATE %s", heap);
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P2-DEFAULT-HEAP-PUBLIC %{public}s", heap);
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P2-DEFAULT-CONST %s", "const-str");
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P2-DEFAULT-INT %d", 42);
    os_log_with_type(lg, OS_LOG_TYPE_INFO, "P2-INFO-ONLY %{public}s", heap);
    os_log_with_type(lg, OS_LOG_TYPE_DEBUG, "P2-DEBUG-ONLY %{public}s", heap);
    char big[2000]; memset(big, 'x', sizeof big - 1); big[sizeof big - 1] = 0;
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P2-DEFAULT-LONG %{public}s END", big);
    printf("pid %d\n", getpid());
    return 0;
}
