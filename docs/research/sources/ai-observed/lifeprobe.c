#include <os/log.h>
#include <stdio.h>
#include <unistd.h>
/* One line at each of Default (the control), Info and Debug, then nothing:
   how long `log show --info --debug` keeps listing each. */
int main(void) {
    os_log_t lg = os_log_create("dev.baud.probe6", "life");
    os_log_with_type(lg, OS_LOG_TYPE_DEFAULT, "P6-DEFAULT-CONTROL %d", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_INFO, "P6-INFO %d", getpid());
    os_log_with_type(lg, OS_LOG_TYPE_DEBUG, "P6-DEBUG %d", getpid());
    printf("pid %d\n", getpid());
    return 0;
}
