const std = @import("std");
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const exe = b.addExecutable(.{
        .name = "ghostty-bench",
        .root_module = b.createModule(.{
            .root_source_file = b.path("bench.zig"),
            .target = target,
            .optimize = optimize,
            .link_libc = true,
        }),
    });
    exe.root_module.linkSystemLibrary("util", .{});
    // ghostty's terminal module, straight from the checkout
    const term_mod = b.createModule(.{
        .root_source_file = b.path("../../../ghostty/src/bench_terminal_root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    // Use ghostty's own options builder rather than reconstructing it, so the
    // module is configured exactly as ghostty configures its own libghostty-vt.
    const TermOpts = @import("tbuild_options.zig").Options;
    const topts: TermOpts = .{
        .artifact = .lib,
        .simd = false,
        .oniguruma = false,
        .c_abi = false,
        .features = .{ .kitty_graphics = false },
        .version = .{ .major = 0, .minor = 0, .patch = 0 },
        .slow_runtime_safety = false,
    };
    topts.add(b, term_mod);
    // the simd helpers reach for a separate `build_options` module
    const bopts = b.addOptions();
    bopts.addOption(bool, "simd", false);
    bopts.addOption(bool, "slow_runtime_safety", false);
    term_mod.addOptions("build_options", bopts);
    // Configure uucode exactly as ghostty does (src/build/SharedDeps.zig:64).
    const uucode_tables = b.dependency("uucode", .{
        .build_config_path = b.path("uucode_config.zig"),
    }).namedLazyPath("tables.zig");
    const uucode_mod = b.dependency("uucode", .{
        .tables_path = uucode_tables,
        .build_config_path = b.path("uucode_config.zig"),
    }).module("uucode");
    term_mod.addImport("uucode", uucode_mod);
    exe.root_module.addImport("terminal", term_mod);
    b.installArtifact(exe);
}
