# File links a program printed over `ssh`

`remote-file-links.raw` is what a real OpenSSH session wrote to its
terminal: 191 bytes, sha256
`2e4e14ca17b0d4dd4a9510f1cfebf6e346b3393574872322ddf333b22f5e067b`.
The program on the other end printed three `OSC 8` links naming files by
this machine's names, then ran `ls --hyperlink=always`:

- `file:///etc/passwd`, the empty host;
- `file://localhost/etc/passwd`;
- `file://here.example/etc/passwd`, the name the tests give this machine;
- `file://remotebox/etc/hosts`, `ls`'s own link, naming the machine it ran
  on.

The remote's pty wrote each newline as `\r\n`, and `ls` ends its links
with `BEL` where `printf` ended them with `ESC \`.

It was captured in one Debian trixie container with no network, OpenSSH
10.0p2 as both client and server over the container's loopback, a key made
for the run:

```sh
cat > Dockerfile <<'D'
FROM debian:trixie
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server openssh-client && rm -rf /var/lib/apt/lists/*
D
docker build -q -t ssh-link-capture .
docker run -i --rm --network none --hostname remotebox ssh-link-capture sh -s <<'S' | base64 -d > remote-file-links.raw
set -e
mkdir -p /run/sshd /root/.ssh
ssh-keygen -q -A
ssh-keygen -q -t ed25519 -N '' -f /tmp/k
cp /tmp/k.pub /root/.ssh/authorized_keys
cat > /tmp/remote.sh <<'R'
printf '\033]8;;file:///etc/passwd\033\\passwd\033]8;;\033\\\n'
printf '\033]8;;file://localhost/etc/passwd\033\\passwd\033]8;;\033\\\n'
printf '\033]8;;file://here.example/etc/passwd\033\\passwd\033]8;;\033\\\n'
ls --hyperlink=always /etc/hosts
R
/usr/sbin/sshd -p 2222 -o ListenAddress=127.0.0.1
n=0
until ssh -tt -p 2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -i /tmp/k root@127.0.0.1 'sh /tmp/remote.sh' > /tmp/cap.bin < /dev/null; do
  n=$((n+1)); [ $n -lt 50 ] || exit 9
done
base64 -w0 /tmp/cap.bin
S
```

`baud-app/src/paths.rs`, `a_real_ssh_sessions_file_links_are_its_machines`,
feeds it as the bytes a window read while `ssh` was in front and requires
every link that names this machine to be refused on a click, while `ssh` is
in front and after it ends, and the remote's own host refused as any other
host is.

## The same names as plain text

`remote-plain-file-uris.raw` is a second session of the same setup, 109
bytes, sha256
`345f00867a9ea080eb6477b7690850f20e0d7e101afd038540e37bf8fb83e04d`.
The program on the other end printed the same four `file://` URIs as
plain text, marked with nothing, one a line. The remote's pty wrote each
newline as `\r\n`. It was captured with the commands above, `remote.sh`
being:

```sh
printf 'file:///etc/passwd\n'
printf 'file://localhost/etc/passwd\n'
printf 'file://here.example/etc/passwd\n'
printf 'file://remotebox/etc/hosts\n'
```

and its output written to `remote-plain-file-uris.raw`.

`baud-app/src/paths.rs`, `a_real_ssh_sessions_plain_file_uris_are_its_machines`,
feeds it as the bytes a window read while `ssh` was in front, reads each URI
back from the text of the cells, and requires the same answers as the
marked links get.
