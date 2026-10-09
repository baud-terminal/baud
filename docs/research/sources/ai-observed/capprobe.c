#include <os/log.h>
#include <string.h>
int main(void) {
    os_log_t lg = os_log_create("dev.baud.probe4", "cap");
    char big[601]; memset(big, 'y', 600); big[600] = 0;
    os_log_with_type(lg, OS_LOG_TYPE_ERROR, "P4-ERROR-600 %{public}s END", big);
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P4-DEFAULT-600 %{public}s END", big);
    os_log_with_type(lg, OS_LOG_TYPE_FAULT, "P4-FAULT-600 %{public}s END", big);
    return 0;
}
