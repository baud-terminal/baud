//! **A ReGIS flood floods what a plain search reaches, whatever its stack
//! holds** (`RegisCanvas::flood`): once the stack of spans to spread into
//! is full, the flood sweeps for an unreached pixel beside a reached one,
//! so a stack of one span reaches what a full one reaches. Each wall
//! pattern below is flooded from its corner and held to a search over the
//! same plane, the hand model.

use baud_domain::{
    FLOOD_STACK, ImagePx, LinePattern, MARK, Negate, RegisBound, RegisCanvas, RegisClip,
    RegisIndex, RegisMicro, RegisPen, RegisPlanes, RegisPoint, RegisSpan, WRITTEN, WriteMode,
};

const SIDE: u16 = 64;

fn pen() -> RegisPen {
    RegisPen {
        mode: WriteMode::Overlay,
        negate: Negate::Off,
        fg: RegisIndex::wrap(2),
        bg: RegisIndex::ZERO,
        planes: RegisPlanes::ALL,
        pattern: LinePattern::SOLID,
        pattern_mul: 1,
        shade: None,
        clip: RegisClip::Off,
        width: 1,
        micro: RegisMicro::NONE,
    }
}

/// Whether `(x, y)` is a wall.
type Wall = fn(usize, usize) -> bool;

/// The wall patterns: a maze of short runs; a lattice, every row of short
/// runs between open rows; a comb of teeth from an open column;
/// serpentines that turn at each end, upright and lying; and walls
/// scattered by a fixed sequence.
fn patterns() -> Vec<(&'static str, Wall)> {
    vec![
        ("maze", |x, y| {
            (x % 4 == 3 && (3 * y + x) % 7 != 1) || (y % 5 == 4 && (x + 2 * y) % 9 != 0)
        }),
        ("lattice", |x, y| x % 2 == 1 && y % 2 == 1),
        ("comb", |x, y| y % 2 == 1 && x % 2 == 0 && x > 0),
        ("serpentine", |x, y| {
            x % 2 == 1 && !((x % 4 == 1 && y == 63) || (x % 4 == 3 && y == 0))
        }),
        ("serpentine lying", |x, y| {
            y % 2 == 1 && !((y % 4 == 1 && x == 63) || (y % 4 == 3 && x == 0))
        }),
        ("scattered", |x, y| {
            let v = ((x * 73_856_093) ^ (y * 19_349_663)) % 1000;
            v < 300 && (x, y) != (0, 0)
        }),
    ]
}

fn plane_of(wall: Wall) -> Vec<u8> {
    let stride = RegisCanvas::stride(ImagePx::of(SIDE));
    let side = usize::from(SIDE);
    let mut plane = vec![0u8; stride * side];
    for y in 0..side {
        for x in 0..side {
            if wall(x, y) {
                plane[y * stride + x] = WRITTEN | 7;
            }
        }
    }
    plane
}

/// The open pixels joined to the corner side by side, found by a plain
/// search over the plane.
fn reachable(plane: &[u8]) -> Vec<bool> {
    let stride = RegisCanvas::stride(ImagePx::of(SIDE));
    let side = usize::from(SIDE);
    let mut seen = vec![false; plane.len()];
    let mut todo = vec![(0usize, 0usize)];
    while let Some((x, y)) = todo.pop() {
        let i = y * stride + x;
        if seen[i] || plane[i] != 0 {
            continue;
        }
        seen[i] = true;
        if x > 0 {
            todo.push((x - 1, y));
        }
        if x + 1 < side {
            todo.push((x + 1, y));
        }
        if y > 0 {
            todo.push((x, y - 1));
        }
        if y + 1 < side {
            todo.push((x, y + 1));
        }
    }
    seen
}

fn flood_with(plane: &[u8], stack_len: usize, bound: RegisBound) -> Vec<u8> {
    let mut plane = plane.to_vec();
    let mut stack = vec![RegisSpan::default(); stack_len];
    let mut c = RegisCanvas::new(&mut plane, ImagePx::of(SIDE), ImagePx::of(SIDE));
    c.flood(RegisPoint { x: 0, y: 0 }, bound, &pen(), &mut stack);
    plane
}

#[test]
fn a_stack_of_one_floods_what_a_full_stack_floods() {
    for (name, wall) in patterns() {
        let plane = plane_of(wall);
        let want = reachable(&plane);
        let open = want.iter().filter(|&&s| s).count();
        assert!(open > 600, "{name}: open enough to wind, {open}");
        for bound in [
            RegisBound::Seed,
            RegisBound::Background,
            RegisBound::Index(RegisIndex::wrap(7)),
        ] {
            let full = flood_with(&plane, FLOOD_STACK, bound);
            let filled: Vec<bool> = full.iter().map(|&b| b == WRITTEN | 2).collect();
            assert_eq!(filled, want, "{name}, {bound:?}");
            for n in [1, 2, 3, 7] {
                let got = flood_with(&plane, n, bound);
                assert_eq!(got, full, "{name}, {bound:?}: a stack of {n}");
            }
            // No mark is left behind.
            assert!(full.iter().all(|&b| b & MARK == 0), "{name}, {bound:?}");
        }
    }
}
