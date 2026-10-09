#include <os/log.h>
#include <string.h>
/* The macros log.h's 256-byte sentences document: os_log_error and os_log_fault
   (-> _os_log_error_impl / _os_log_fault_impl), and os_log_info. */
int main(void) {
    os_log_t lg = os_log_create("dev.baud.probe7", "cap2");
    char big[601]; memset(big, 'z', 600); big[600] = 0;
    os_log_error(lg, "P7-OSLOGERROR-600 %{public}s END", big);
    os_log_fault(lg, "P7-OSLOGFAULT-600 %{public}s END", big);
    os_log_info(lg, "P7-OSLOGINFO-600 %{public}s END", big);
    os_log(lg, "P7-OSLOG-CONTROL short");
    return 0;
}
