// Re-export ghostty's terminal from inside its own source tree so its
// relative imports (../quirks.zig, ../simd/main.zig) resolve.
pub usingnamespace @import("../ghostty/src/terminal/main.zig");
