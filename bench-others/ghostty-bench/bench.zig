// Feed ghostty's terminal the same payloads through a real PTY.
const std = @import("std");
const terminal = @import("terminal").T;
const posix = std.posix;
const c = @cImport({ @cInclude("pty.h"); @cInclude("termios.h"); @cInclude("unistd.h");
                     @cInclude("signal.h"); @cInclude("sys/wait.h"); @cInclude("time.h"); });

const COLS: usize = 200;
const ROWS: usize = 50;

fn throughPty(io: std.Io, alloc: std.mem.Allocator, payload: []const u8, total: usize) !f64 {
    var m: c_int = 0;
    var s: c_int = 0;
    _ = c.openpty(&m, &s, null, null, null);
    var t: c.termios = undefined;
    _ = c.tcgetattr(s, &t);
    c.cfmakeraw(&t);
    _ = c.tcsetattr(s, c.TCSANOW, &t);

    const pid = c.fork();
    if (pid == 0) {
        _ = c.close(m);
        var sent: usize = 0;
        while (sent < total) {
            var o: usize = 0;
            while (o < payload.len) {
                const w = c.write(s, payload.ptr + o, payload.len - o);
                if (w <= 0) { _ = c.close(s); c._exit(0); }
                o += @intCast(w);
            }
            sent += payload.len;
        }
        _ = c.close(s);
        c._exit(0);
    }
    _ = c.close(s);

    var term = try terminal.Terminal.init(io, alloc, .{
        .cols = COLS, .rows = ROWS,
    });
    defer term.deinit(alloc);
    var strm: terminal.TerminalStream = .init(.{
        .handler = .{ .terminal = &term },
        .allocator = alloc,
    });

    var buf: [4096]u8 = undefined;
    var got: usize = 0;
    var ts0: c.struct_timespec = undefined;
    _ = c.clock_gettime(c.CLOCK_MONOTONIC, &ts0);
    while (true) {
        const r = c.read(m, &buf, 4096);
        if (r <= 0) break;
        const n: usize = @intCast(r);
        try strm.nextSlice(buf[0..n]);
        got += n;
        if (got >= total) break;
    }
    var ts1: c.struct_timespec = undefined;
    _ = c.clock_gettime(c.CLOCK_MONOTONIC, &ts1);
    const ns: u64 = @intCast((ts1.tv_sec - ts0.tv_sec) * 1_000_000_000 + (ts1.tv_nsec - ts0.tv_nsec));
    _ = c.kill(pid, 9);
    var st: c_int = 0;
    _ = c.waitpid(pid, &st, 0);
    _ = c.close(m);
    return @as(f64, @floatFromInt(got)) / (@as(f64, @floatFromInt(ns)) / 1e9) / 1048576.0;
}

pub fn main() !void {
    var dbg = std.heap.DebugAllocator(.{}){};
    const alloc = dbg.allocator();
    var threaded: std.Io.Threaded = .init(alloc, .{});
    defer threaded.deinit();
    const io = threaded.io();
    const total: usize = 32 << 20;

    // fixed buffers; the Zig ArrayList API moved between releases and the
    // payload shape is what matters, not the container.
    const CAP: usize = (1 << 19);
    const sbuf = try alloc.alloc(u8, CAP);
    defer alloc.free(sbuf);
    const tbuf = try alloc.alloc(u8, CAP);
    defer alloc.free(tbuf);

    const W = [_][]const u8{ "static", "inline", "struct", "return", "const", "size_t", "render", "parse" };
    var sn: usize = 0;
    var col: usize = 0;
    while (sn < CAP - 100) {
        if (col > 70) { sbuf[sn] = '\r'; sbuf[sn + 1] = '\n'; sn += 2; col = 0; continue; }
        if (sn % 37 == 0) { @memcpy(sbuf[sn .. sn + 5], "\x1b[32m"); sn += 5; continue; }
        const w = W[sn % 8];
        @memcpy(sbuf[sn .. sn + w.len], w); sn += w.len; col += w.len;
        sbuf[sn] = ' '; sn += 1; col += 1;
    }
    var tn: usize = 0;
    var sq: u64 = 0;
    while (tn < CAP - 200) {
        var tmp: [32]u8 = undefined;
        const hdr = try std.fmt.bufPrint(&tmp, "\x1b[{d};{d}H", .{ sq % 50 + 1, 1 + (sq * 37) % 120 });
        @memcpy(tbuf[tn .. tn + hdr.len], hdr); tn += hdr.len;
        var k: usize = 0;
        while (k < 6) : (k += 1) { @memcpy(tbuf[tn .. tn + 11], "pub struct "); tn += 11; }
        @memcpy(tbuf[tn .. tn + 3], "\x1b[K"); tn += 3;
        sq += 1;
    }
    const stream_items = sbuf[0..sn];
    const tui_items = tbuf[0..tn];

    var bs: f64 = 0;
    var bt: f64 = 0;
    var i: usize = 0;
    while (i < 5) : (i += 1) {
        const a = try throughPty(io, alloc, stream_items, total);
        if (a > bs) bs = a;
        const b = try throughPty(io, alloc, tui_items, total);
        if (b > bt) bt = b;
    }
    var out = std.fs.File.stdout().writer(&.{});
    try out.interface.print("  {s:<28} {d:>9.1}    {d:>9.1}\n", .{ "ghostty (full terminal)", bs, bt });
}
