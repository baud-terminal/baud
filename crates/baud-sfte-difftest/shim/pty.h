/* Stub for a header macOS does not have.
 *
 * sfte's implementation include block (sfte_dev.h:1502-1514 at the pinned
 * commit) pulls <pty.h> unconditionally -- no macro suppresses it -- for
 * forkpty, which only the POSIX pty backend calls. That backend is compiled
 * out here (SFTE_NO_POSIX), so nothing in this build references anything the
 * real header declares; this file exists so the include resolves. build.rs
 * puts this directory on the include path off Linux only, because on Linux
 * the real <pty.h> exists and must not be shadowed.
 */
