# The tty layer — why a PTY read is 1024 bytes on Darwin and 4095 on Linux

> **Group:** E · **Sources:** `[[source: linux-n-tty]]`, `[[source: linux-tty-buffer]]`, `[[source: linux-pty]]`, `[[source: xnu-tty]]`, `[[source: xnu-tty-h]]`, `[[source: xnu-tty-dev]]`, `[[source: xnu-tty-subr]]` · **Status:** partial

## Why this note exists

Group E was declared in `MANIFEST.toml` when the corpus was set up and was
**empty**, exactly as Group D was before `d-memory-hierarchy.md`. Every claim
this project makes about the transport rested on its own measurement and on
an explanation from memory:

* `READ_CHUNK = 4096` "because `N_TTY_BUF_SIZE` caps `read(2)` at 4095 bytes";
* "the tty hands back **1024**" on Darwin, so a PTY row measures the tty and
  not the parser -- 198.6 MB/s for a consumer that only reads and discards;
* `splice()` on the PTY "goes through the ldisc ring anyway";
* eBPF "cannot skip the `queue_work()` hop".

The measurements are sound and are kept in `MEASUREMENTS.md` and
`baud-bench/benches/transport.rs`. What this note adds is the mechanism, read
from the two kernels at a pinned commit each -- and the first thing reading
them did was **correct the mechanism this repo would have named for Darwin**.

## Linux: a 4096-byte ring that keeps one byte free

`[[source: linux-n-tty]]` line 59: `#define N_TTY_BUF_SIZE 4096`. It is defined
in `n_tty.c` itself, not in `include/linux/tty.h`, which is worth knowing
because a grep of the header at this commit finds nothing and reads as the
constant having gone.

The 4095 is `n_tty_receive_buf_common`:

```c
room = N_TTY_BUF_SIZE - (ldata->read_head - tail);
if (I_PARMRK(tty))
        room = DIV_ROUND_UP(room, 3);
room--;
```

`read_buf` is a ring indexed through `MASK(x) ((x) & (N_TTY_BUF_SIZE - 1))`,
and the `room--` keeps one slot empty so a full ring and an empty one differ.
So at most **4095 bytes are ever buffered for a reader**, and a `read(2)`
offering more receives at most that. `docs/architecture.md`'s "offering 1 MiB
gets you 4095" is this line.

**The path from a pty write to that ring has a workqueue in it.**
`[[source: linux-pty]]`'s `pty_write` is one call --
`tty_insert_flip_string_and_push_buffer(to->port, buf, c)` -- and
`[[source: linux-tty-buffer]]` shows what push means:

```c
static bool tty_buffer_queue_work(struct tty_bufhead *buf)
{
        struct workqueue_struct *flip_wq = READ_ONCE(buf->flip_wq);
        return queue_work(flip_wq ?: system_dfl_wq, &buf->work);
}
```

with `flush_to_ldisc` as the work. The bytes reach the line discipline on a
kernel worker, not in the writer's syscall. That is the hop eBPF can measure and cannot remove (CONTRIBUTING.md, "Things already tried that lost"), and it is why `splice()` lost to
`write(2)`: there is no route around the flip buffer, so splicing adds a pipe
in front of the same ring.

## Darwin: a 1024-byte clist, and it is not `TTYHOG`

The obvious explanation for 1024 is `TTYHOG`, which `[[source: xnu-tty-h]]`
line 181 defines as 1024. **It is the wrong constant.** `TTYHOG` bounds the
*input* queues -- `[[source: xnu-tty-dev]]`'s `ptcwrite` feeds `ttyinput` one
byte at a time and blocks when `t_rawq.c_cc + t_canq.c_cc >= TTYHOG - 2`. That
is the master-to-replica direction: the bound an emulator's *keystrokes* see,
and not the one its reads see.

What an emulator reads is the replica's **output** queue, drained by
`ptcread`:

```c
while (uio_resid(uio) > 0 && error == 0) {
        cc = q_to_b(&tp->t_outq, (u_char *)buf, MIN((int)uio_resid(uio), BUFSIZ));
        if (cc <= 0)
                break;
        error = uiomove(buf, cc, uio);
}
```

(`BUFSIZ` is 100 and local to that file -- the copy step, not the bound.) One
`read(2)` returns whatever `t_outq` holds, and what it can hold is fixed at
allocation. `[[source: xnu-tty]]`'s `ttymalloc`:

```c
/* XXX: default to TTYCLSIZE(1024) chars for now */
clalloc(&tp->t_rawq, TTYCLSIZE, 1);
clalloc(&tp->t_canq, TTYCLSIZE, 1);
/* output queue doesn't need quoting */
clalloc(&tp->t_outq, TTYCLSIZE, 0);
```

`[[source: xnu-tty-h]]` line 105: `#define TTYCLSIZE 1024`. And
`[[source: xnu-tty-subr]]` is what makes that a hard bound rather than a
default: `clalloc` sets `c_cn = size`, and `b_to_q` fills `while (count > 0 &&
clp->c_cc < clp->c_cn)` -- a clist is a fixed ring that cannot be overfilled,
and a writer offering more keeps the remainder. `ttwrite` also stops at
`t_hiwat`, which `ttsetwater` derives from the line speed (a pty opens at
`TTYDEF_SPEED`, 9600) and clamps to `TTMAXHIWAT`; that mark is softer than the
capacity and the capacity is what a reader observes.

So both figures have the same shape: **a fixed ring, drained per read.** Linux
keeps one byte of slack in a 4096-byte ring and hands back 4095; Darwin fills a
1024-byte clist to capacity and hands back 1024. `READ_CHUNK = 4096` is right
on both, because it is the larger of the two and the smaller host simply
receives less per syscall -- 49,152 reads for 48 MB, which is the measurement CONTRIBUTING.md records (`READ_CHUNK`).

**Same number, different constant, and only one of them is on the path.**
`TTYHOG` and `TTYCLSIZE` are both 1024 in the same header, so a note that
named `TTYHOG` would have been right by coincidence and wrong the day either
moved. That is what pinning the *source* rather than the *number* buys, and it
is the reason this group is filled with kernel files rather than with a
sentence.

## What this group does not yet pin

* **The pipe side.** `transport.rs` measures a pipe at 6144 MB/s returning
  4096 bytes a read; that is Darwin's pipe implementation
  (`bsd/kern/sys_pipe.c`) and it is not fetched. The claim is measured, not
  explained.
* **Scheduling.** "`SCHED_FIFO` -- p99 50% worse" is a prototype figure
  (`git show d8262f5:README.md`, line 88) with no kernel source behind it: a
  result, not a mechanism, and left as such. The RTIC entry the project's rules carried until c1d710c had no measurement behind it either.
* **`TTHIWAT` in practice.** `t_hiwat` at 9600 baud computes to
  `roundup(1216, CBSIZE)`, above the clist capacity, which is why the capacity
  binds. That arithmetic is from the source and is not separately measured;
  a producer that writes in chunks smaller than 1024 would show whether the
  reader ever sees a partial ring.
