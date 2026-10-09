#include <os/log.h>
void baud_os_log(os_log_t log, os_log_type_t type, const char *message) {
    os_log_with_type(log, type, "%{public}s", message);
}
