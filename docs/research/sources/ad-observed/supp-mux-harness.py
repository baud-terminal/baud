# Spawn a command on a pty (outer terminal stand-in), collect every byte it writes.
import os, pty, sys, time, select, struct, fcntl, termios, subprocess, json
out_path = sys.argv[1]; dur = float(sys.argv[2]); rows, cols = int(sys.argv[3]), int(sys.argv[4])
cmd = sys.argv[5:]
side = os.environ.get('SIDECAR')  # shell command run once at SIDECAR_AT seconds
side_at = float(os.environ.get('SIDECAR_AT', '0'))
pid, fd = pty.fork()
if pid == 0:
    os.execvpe(cmd[0], cmd, os.environ)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
buf = bytearray(); t0 = time.time(); side_done = False
while time.time() - t0 < dur:
    r, _, _ = select.select([fd], [], [], 0.1)
    if r:
        try:
            d = os.read(fd, 65536)
        except OSError:
            break
        if not d: break
        buf += d
        # answer DA1/DA2/XTVERSION-ish queries minimally so the program does not stall
        if b'\x1b[c' in d or b'\x1b[0c' in d:
            os.write(fd, b'\x1b[?62;22c')
        if b'\x1b[>c' in d or b'\x1b[>0c' in d:
            os.write(fd, b'\x1b[>0;0;0c')
    if side and not side_done and time.time() - t0 >= side_at:
        subprocess.run(side, shell=True); side_done = True
open(out_path, 'wb').write(buf)
try:
    os.kill(pid, 15)
except Exception: pass
print('bytes', len(buf))
