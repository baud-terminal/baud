/* Stub for a header macOS does not have.
 *
 * sfte includes <sys/timerfd.h> unconditionally (sfte_dev.h:1512 at the
 * pinned commit) for timerfd_create/timerfd_settime, which only the Wayland
 * backend's key-repeat timer calls. That backend is compiled out here
 * (SFTE_CUSTOM_BACKEND), so nothing references the symbols; this file exists
 * so the include resolves. Off Linux only -- see ../pty.h.
 */
