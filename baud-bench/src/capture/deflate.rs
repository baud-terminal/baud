//! `zlib.compress(data, 6)`, byte for byte: zlib 1.3.1's `deflate_slow` and
//! tree builder at level 6, window 15, memLevel 8, default strategy, with all
//! the input in one call and `Z_FINISH`.
//!
//! **Why here and in Rust.** Which bytes a deflate writes is the deflater's
//! choice (`[[source: rfc1951-deflate]]` fixes only what they decode to), and
//! `gen-yazi-files` writes its PNGs in the bytes the script's zlib chose. The
//! crates that link C engines live outside the workspace (`tools/`), so this
//! crate does not take `-lz`, and a Rust port gives the same bytes on a host
//! whatever zlib it has. It is a port of two of zlib's files, not a general
//! deflater: one level, no resumption, nothing but what that path reaches.
//!
//! ```text
//! zlib 1.3.1 (deflate.c, trees.c), this file altered into Rust from them.
//! Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler
//!
//! This software is provided 'as-is', without any express or implied
//! warranty.  In no event will the authors be held liable for any damages
//! arising from the use of this software.
//!
//! Permission is granted to anyone to use this software for any purpose,
//! including commercial applications, and to alter it and redistribute it
//! freely, subject to the following restrictions:
//!
//! 1. The origin of this software must not be misrepresented; you must not
//!    claim that you wrote the original software. If you use this software
//!    in a product, an acknowledgment in the product documentation would be
//!    appreciated but is not required.
//! 2. Altered source versions must be plainly marked as such, and must not be
//!    misrepresented as being the original software.
//! 3. This notice may not be removed or altered from any source distribution.
//! ```

const W_SIZE: usize = 32768;
const W_MASK: usize = W_SIZE - 1;
const WINDOW: usize = 2 * W_SIZE;
const HASH_SIZE: usize = 32768;
const HASH_MASK: usize = HASH_SIZE - 1;
const HASH_SHIFT: usize = 5;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
const MAX_DIST: usize = W_SIZE - MIN_LOOKAHEAD;
const TOO_FAR: usize = 4096;
// Level 6's row of `configuration_table`.
const GOOD_LENGTH: usize = 8;
const MAX_LAZY: usize = 16;
const NICE_LENGTH: usize = 128;
const MAX_CHAIN: usize = 128;
/// `lit_bufsize`, and a block ends one symbol short of it.
const LIT_BUFSIZE: usize = 16384;

const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;
const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const SMALLEST: usize = 1;

const EXTRA_LBITS: [u8; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [u8; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [u8; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// A window position or a count, for a field the audit asks to be named.
#[derive(Clone, Copy, Default)]
struct N(usize);

/// A code's frequency and then its code, the union zlib makes of them.
#[derive(Clone, Copy, Default)]
struct FreqCode(u16);

/// A code's parent and then its length, the union zlib makes of them.
#[derive(Clone, Copy, Default)]
struct DadLen(u16);

/// A code's `ct_data`.
#[derive(Clone, Copy, Default)]
struct Ct {
    fc: FreqCode,
    dl: DadLen,
}

/// A code as an index.
fn idx(v: u16) -> usize {
    usize::from(v)
}

/// A value zlib's arithmetic keeps in a `ush`.
fn ush(v: usize) -> u16 {
    u16::try_from(v).expect("fits a ush")
}

/// `bi_reverse`: the first `len` bits of `code`, reversed.
fn reverse(code: usize, len: usize) -> u16 {
    let (mut code, mut res) = (code, 0usize);
    for _ in 0..len {
        res = (res << 1) | (code & 1);
        code >>= 1;
    }
    ush(res)
}

/// `gen_codes`.
fn gen_codes(tree: &mut [Ct], max_code: usize, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next = [0usize; MAX_BITS + 1];
    let mut code = 0usize;
    for bits in 1..=MAX_BITS {
        code = (code + idx(bl_count[bits - 1])) << 1;
        next[bits] = code & 0xffff;
    }
    for ct in tree.iter_mut().take(max_code + 1) {
        let len = idx(ct.dl.0);
        if len != 0 {
            ct.fc.0 = reverse(next[len], len);
            next[len] += 1;
        }
    }
}

/// `tr_static_init`'s tables.
struct Statics {
    ltree: [Ct; L_CODES + 2],
    dtree: [Ct; D_CODES],
    length_code: [u8; MAX_MATCH - MIN_MATCH + 1],
    dist_code: [u8; 512],
    base_length: [usize; LENGTH_CODES],
    base_dist: [usize; D_CODES],
}

impl Statics {
    fn new() -> Statics {
        let mut s = Statics {
            ltree: [Ct::default(); L_CODES + 2],
            dtree: [Ct::default(); D_CODES],
            length_code: [0; MAX_MATCH - MIN_MATCH + 1],
            dist_code: [0; 512],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
        };
        let mut length = 0;
        for (code, extra) in EXTRA_LBITS.iter().enumerate().take(LENGTH_CODES - 1) {
            s.base_length[code] = length;
            for _ in 0..(1usize << extra) {
                s.length_code[length] = u8::try_from(code).expect("a length code");
                length += 1;
            }
        }
        s.length_code[length - 1] = u8::try_from(LENGTH_CODES - 1).expect("a length code");
        let mut dist = 0;
        for (code, extra) in EXTRA_DBITS.iter().enumerate().take(16) {
            s.base_dist[code] = dist;
            for _ in 0..(1usize << extra) {
                s.dist_code[dist] = u8::try_from(code).expect("a distance code");
                dist += 1;
            }
        }
        dist >>= 7;
        for (code, extra) in EXTRA_DBITS.iter().enumerate().skip(16) {
            s.base_dist[code] = dist << 7;
            for _ in 0..(1usize << (extra - 7)) {
                s.dist_code[256 + dist] = u8::try_from(code).expect("a distance code");
                dist += 1;
            }
        }
        let mut bl_count = [0u16; MAX_BITS + 1];
        for (n, ct) in s.ltree.iter_mut().enumerate() {
            let len = match n {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
            ct.dl.0 = ush(len);
            bl_count[len] += 1;
        }
        gen_codes(&mut s.ltree, L_CODES + 1, &bl_count);
        for (n, ct) in s.dtree.iter_mut().enumerate() {
            ct.dl.0 = 5;
            ct.fc.0 = reverse(n, 5);
        }
        s
    }

    fn d_code(&self, dist: usize) -> usize {
        usize::from(if dist < 256 {
            self.dist_code[dist]
        } else {
            self.dist_code[256 + (dist >> 7)]
        })
    }
}

/// The heap and the running bit lengths `build_tree` shares.
struct Heap {
    heap: [usize; HEAP_SIZE],
    len: N,
    max: N,
    depth: [u8; HEAP_SIZE],
    bl_count: [u16; MAX_BITS + 1],
    /// `opt_len` and `static_len`, which wrap as an unsigned long does: the
    /// forced-code step in `build_tree` takes them below zero for a moment.
    opt_len: Bits,
    static_len: Bits,
}

/// A count of bits, wrapping.
#[derive(Clone, Copy, Default)]
struct Bits(u64);

impl Bits {
    fn add(&mut self, v: u64) {
        self.0 = self.0.wrapping_add(v);
    }

    fn sub(&mut self, v: u64) {
        self.0 = self.0.wrapping_sub(v);
    }

    /// `(len + 3 + 7) >> 3`.
    fn bytes(self) -> u64 {
        self.0.wrapping_add(10) >> 3
    }
}

/// `smaller`.
fn smaller(tree: &[Ct], n: usize, m: usize, depth: &[u8; HEAP_SIZE]) -> bool {
    tree[n].fc.0 < tree[m].fc.0 || (tree[n].fc.0 == tree[m].fc.0 && depth[n] <= depth[m])
}

/// `pqdownheap`.
fn pqdownheap(h: &mut Heap, tree: &[Ct], start: usize) {
    let mut k = start;
    let v = h.heap[k];
    let mut j = k << 1;
    while j <= h.len.0 {
        if j < h.len.0 && smaller(tree, h.heap[j + 1], h.heap[j], &h.depth) {
            j += 1;
        }
        if smaller(tree, v, h.heap[j], &h.depth) {
            break;
        }
        h.heap[k] = h.heap[j];
        k = j;
        j <<= 1;
    }
    h.heap[k] = v;
}

/// A tree's fixed description: `static_tree_desc`.
#[derive(Clone, Copy)]
struct Desc<'a> {
    stree: Option<&'a [Ct]>,
    extra: &'a [u8],
    base: N,
    elems: N,
    max_length: N,
}

/// `gen_bitlen`.
fn gen_bitlen(h: &mut Heap, tree: &mut [Ct], max_code: usize, d: Desc<'_>) {
    let max_length = d.max_length.0;
    h.bl_count = [0; MAX_BITS + 1];
    let mut overflow = 0i32;
    tree[h.heap[h.max.0]].dl.0 = 0;
    let mut i = h.max.0 + 1;
    while i < HEAP_SIZE {
        let n = h.heap[i];
        i += 1;
        let mut bits = idx(tree[idx(tree[n].dl.0)].dl.0) + 1;
        if bits > max_length {
            bits = max_length;
            overflow += 1;
        }
        tree[n].dl.0 = ush(bits);
        if n > max_code {
            continue;
        }
        h.bl_count[bits] += 1;
        let xbits = if n >= d.base.0 {
            usize::from(d.extra[n - d.base.0])
        } else {
            0
        };
        let f = u64::from(tree[n].fc.0);
        h.opt_len.add(f * u64::from(ush(bits + xbits)));
        if let Some(stree) = d.stree {
            h.static_len
                .add(f * u64::from(ush(idx(stree[n].dl.0) + xbits)));
        }
    }
    if overflow == 0 {
        return;
    }
    while overflow > 0 {
        let mut bits = max_length - 1;
        while h.bl_count[bits] == 0 {
            bits -= 1;
        }
        h.bl_count[bits] -= 1;
        h.bl_count[bits + 1] += 2;
        h.bl_count[max_length] -= 1;
        overflow -= 2;
    }
    let mut at = HEAP_SIZE;
    for bits in (1..=max_length).rev() {
        let mut n = h.bl_count[bits];
        while n != 0 {
            at -= 1;
            let m = h.heap[at];
            if m > max_code {
                continue;
            }
            if idx(tree[m].dl.0) != bits {
                let was = u64::from(tree[m].dl.0);
                let f = u64::from(tree[m].fc.0);
                h.opt_len
                    .add(u64::from(ush(bits)).wrapping_sub(was).wrapping_mul(f));
                tree[m].dl.0 = ush(bits);
            }
            n -= 1;
        }
    }
}

/// `build_tree`: the largest code with a frequency.
fn build_tree(h: &mut Heap, tree: &mut [Ct], d: Desc<'_>) -> usize {
    let elems = d.elems.0;
    let mut max_code = None;
    h.len = N(0);
    h.max = N(HEAP_SIZE);
    for (n, ct) in tree.iter_mut().enumerate().take(elems) {
        if ct.fc.0 != 0 {
            h.len.0 += 1;
            h.heap[h.len.0] = n;
            max_code = Some(n);
            h.depth[n] = 0;
        } else {
            ct.dl.0 = 0;
        }
    }
    let mut max = max_code;
    while h.len.0 < 2 {
        // `max_code < 2 ? ++max_code : 0`, from -1 when nothing was seen.
        let node = match max {
            None => {
                max = Some(0);
                0
            }
            Some(m) if m < 2 => {
                max = Some(m + 1);
                m + 1
            }
            Some(_) => 0,
        };
        h.len.0 += 1;
        h.heap[h.len.0] = node;
        tree[node].fc.0 = 1;
        h.depth[node] = 0;
        h.opt_len.sub(1);
        if let Some(stree) = d.stree {
            h.static_len.sub(u64::from(stree[node].dl.0));
        }
    }
    let max_code = max.expect("two codes forced");
    let mut n = h.len.0 / 2;
    while n >= 1 {
        pqdownheap(h, tree, n);
        n -= 1;
    }
    let mut node = elems;
    loop {
        // pqremove
        let n = h.heap[SMALLEST];
        h.heap[SMALLEST] = h.heap[h.len.0];
        h.len.0 -= 1;
        pqdownheap(h, tree, SMALLEST);
        let m = h.heap[SMALLEST];
        h.max.0 -= 1;
        h.heap[h.max.0] = n;
        h.max.0 -= 1;
        h.heap[h.max.0] = m;
        tree[node].fc.0 = tree[n].fc.0.wrapping_add(tree[m].fc.0);
        h.depth[node] = h.depth[n].max(h.depth[m]) + 1;
        tree[n].dl.0 = ush(node);
        tree[m].dl.0 = ush(node);
        h.heap[SMALLEST] = node;
        node += 1;
        pqdownheap(h, tree, SMALLEST);
        if h.len.0 < 2 {
            break;
        }
    }
    h.max.0 -= 1;
    h.heap[h.max.0] = h.heap[SMALLEST];
    gen_bitlen(h, tree, max_code, d);
    let counts = h.bl_count;
    gen_codes(tree, max_code, &counts);
    max_code
}

/// The output and its bit buffer.
struct Out {
    bytes: Vec<u8>,
    acc: Bits,
    have: N,
}

impl Out {
    /// `send_bits`.
    fn send(&mut self, value: u64, length: N) {
        self.acc.0 |= value << self.have.0;
        self.have.0 += length.0;
        while self.have.0 >= 8 {
            self.bytes.push(self.acc.0.to_le_bytes()[0]);
            self.acc.0 >>= 8;
            self.have.0 -= 8;
        }
    }

    /// `send_code`.
    fn code(&mut self, ct: Ct) {
        self.send(u64::from(ct.fc.0), N(idx(ct.dl.0)));
    }

    /// `bi_windup`.
    fn windup(&mut self) {
        if self.have.0 > 0 {
            self.bytes.push(self.acc.0.to_le_bytes()[0]);
        }
        self.acc = Bits(0);
        self.have = N(0);
    }
}

/// What `scan_tree` and `send_tree` both walk: each run of code lengths, as
/// `f(curlen, count, prevlen, min_count)`.
fn walk_lengths(tree: &[Ct], max: usize, mut f: impl FnMut(usize, usize, Option<usize>, usize)) {
    let mut prev = None;
    let mut next = idx(tree[0].dl.0);
    let mut count = 0;
    let (mut max_count, mut min_count) = if next == 0 { (138, 3) } else { (7, 4) };
    for n in 0..=max {
        let cur = next;
        next = idx(tree[n + 1].dl.0);
        count += 1;
        if count < max_count && cur == next {
            continue;
        }
        f(cur, count, prev, min_count);
        count = 0;
        prev = Some(cur);
        (max_count, min_count) = if next == 0 {
            (138, 3)
        } else if cur == next {
            (6, 3)
        } else {
            (7, 4)
        };
    }
}

/// `scan_tree`.
fn scan_tree(bl: &mut [Ct], tree: &mut [Ct], max_code: usize) {
    tree[max_code + 1].dl.0 = 0xffff;
    walk_lengths(tree, max_code, |cur, count, prev, min_count| {
        if count < min_count {
            bl[cur].fc.0 += ush(count);
        } else if cur != 0 {
            if Some(cur) != prev {
                bl[cur].fc.0 += 1;
            }
            bl[REP_3_6].fc.0 += 1;
        } else if count <= 10 {
            bl[REPZ_3_10].fc.0 += 1;
        } else {
            bl[REPZ_11_138].fc.0 += 1;
        }
    });
}

/// `send_tree`.
fn send_tree(out: &mut Out, bl: &[Ct], tree: &[Ct], max_code: usize) {
    walk_lengths(tree, max_code, |cur, count, prev, min_count| {
        let mut count = count;
        if count < min_count {
            for _ in 0..count {
                out.code(bl[cur]);
            }
        } else if cur != 0 {
            if Some(cur) != prev {
                out.code(bl[cur]);
                count -= 1;
            }
            out.code(bl[REP_3_6]);
            out.send(u64::from(ush(count - 3)), N(2));
        } else if count <= 10 {
            out.code(bl[REPZ_3_10]);
            out.send(u64::from(ush(count - 3)), N(3));
        } else {
            out.code(bl[REPZ_11_138]);
            out.send(u64::from(ush(count - 11)), N(7));
        }
    });
}

/// The compressor's state.
struct State<'a> {
    input: &'a [u8],
    read: N,
    window: Vec<u8>,
    head: Vec<u16>,
    prev: Vec<u16>,
    ins_h: N,
    strstart: N,
    lookahead: N,
    match_start: N,
    match_length: N,
    prev_length: N,
    /// May fall below zero: a block longer than the window has lost its start.
    block_start: i64,
    syms: Vec<(u16, u8)>,
    dyn_l: [Ct; HEAP_SIZE],
    dyn_d: [Ct; 2 * D_CODES + 1],
    bl: [Ct; 2 * BL_CODES + 1],
    h: Heap,
    out: Out,
    st: Statics,
}

/// `UPDATE_HASH`.
fn update_hash(h: usize, c: u8) -> usize {
    ((h << HASH_SHIFT) ^ usize::from(c)) & HASH_MASK
}

/// `INSERT_STRING`: the chain's head before this string.
fn insert_string(s: &mut State<'_>, at: usize) -> usize {
    s.ins_h.0 = update_hash(s.ins_h.0, s.window[at + MIN_MATCH - 1]);
    let head = s.head[s.ins_h.0];
    s.prev[at & W_MASK] = head;
    s.head[s.ins_h.0] = ush(at);
    idx(head)
}

/// `slide_hash`.
fn slide_hash(s: &mut State<'_>) {
    for p in s.head.iter_mut().chain(s.prev.iter_mut()) {
        *p = ush(idx(*p).saturating_sub(W_SIZE));
    }
}

/// `fill_window`, the input all in one call.
fn fill_window(s: &mut State<'_>) {
    loop {
        let mut more = WINDOW - s.lookahead.0 - s.strstart.0;
        if s.strstart.0 >= W_SIZE + MAX_DIST {
            s.window.copy_within(W_SIZE..W_SIZE + (W_SIZE - more), 0);
            s.match_start.0 = s.match_start.0.wrapping_sub(W_SIZE);
            s.strstart.0 -= W_SIZE;
            s.block_start -= i64::try_from(W_SIZE).expect("the window's size");
            slide_hash(s);
            more += W_SIZE;
        }
        let left = s.input.len() - s.read.0;
        if left == 0 {
            break;
        }
        let n = left.min(more);
        let to = s.strstart.0 + s.lookahead.0;
        s.window[to..to + n].copy_from_slice(&s.input[s.read.0..s.read.0 + n]);
        s.read.0 += n;
        s.lookahead.0 += n;
        if s.lookahead.0 >= MIN_MATCH {
            let at = s.strstart.0;
            s.ins_h.0 = update_hash(usize::from(s.window[at]), s.window[at + 1]);
        }
        if s.lookahead.0 >= MIN_LOOKAHEAD || s.read.0 == s.input.len() {
            break;
        }
    }
}

/// `longest_match`.
fn longest_match(s: &mut State<'_>, first: usize) -> usize {
    let scan = s.strstart.0;
    let mut chain = MAX_CHAIN;
    let mut best = s.prev_length.0;
    let mut nice = NICE_LENGTH;
    let limit = scan.saturating_sub(MAX_DIST);
    if s.prev_length.0 >= GOOD_LENGTH {
        chain >>= 2;
    }
    if nice > s.lookahead.0 {
        nice = s.lookahead.0;
    }
    let win = &s.window;
    let mut end1 = win[scan + best - 1];
    let mut end = win[scan + best];
    let mut cur = first;
    loop {
        if win[cur + best] == end
            && win[cur + best - 1] == end1
            && win[cur] == win[scan]
            && win[cur + 1] == win[scan + 1]
        {
            // The third byte is not compared: the hashes being equal, it
            // cannot differ, and zlib does not look.
            let mut k = 2;
            loop {
                k += 1;
                if win[scan + k] != win[cur + k] || k == MAX_MATCH {
                    break;
                }
            }
            if k > best {
                s.match_start.0 = cur;
                best = k;
                if k >= nice {
                    break;
                }
                end1 = win[scan + best - 1];
                end = win[scan + best];
            }
        }
        cur = idx(s.prev[cur & W_MASK]);
        chain -= 1;
        if cur <= limit || chain == 0 {
            break;
        }
    }
    best.min(s.lookahead.0)
}

/// `init_block`.
fn init_block(s: &mut State<'_>) {
    for ct in &mut s.dyn_l[..L_CODES] {
        ct.fc.0 = 0;
    }
    for ct in &mut s.dyn_d[..D_CODES] {
        ct.fc.0 = 0;
    }
    for ct in &mut s.bl[..BL_CODES] {
        ct.fc.0 = 0;
    }
    s.dyn_l[END_BLOCK].fc.0 = 1;
    s.h.opt_len = Bits(0);
    s.h.static_len = Bits(0);
    s.syms.clear();
}

/// `compress_block`.
fn compress_block(out: &mut Out, st: &Statics, syms: &[(u16, u8)], ltree: &[Ct], dtree: &[Ct]) {
    for (dist, lc) in syms {
        let lc = usize::from(*lc);
        if *dist == 0 {
            out.code(ltree[lc]);
            continue;
        }
        let code = usize::from(st.length_code[lc]);
        out.code(ltree[code + LITERALS + 1]);
        let extra = usize::from(EXTRA_LBITS[code]);
        if extra != 0 {
            out.send(u64::from(ush(lc - st.base_length[code])), N(extra));
        }
        let dist = idx(*dist) - 1;
        let code = st.d_code(dist);
        out.code(dtree[code]);
        let extra = usize::from(EXTRA_DBITS[code]);
        if extra != 0 {
            out.send(u64::from(ush(dist - st.base_dist[code])), N(extra));
        }
    }
    out.code(ltree[END_BLOCK]);
}

/// zlib's block flush and `FLUSH_BLOCK_ONLY`: the block ending at `strstart`,
/// the last one when `last`.
fn flush_block(s: &mut State<'_>, last: End) {
    let last_bit = u64::from(last == End::Last);
    let start = usize::try_from(s.block_start).ok();
    let stored = usize::try_from(i64::try_from(s.strstart.0).expect("a position") - s.block_start)
        .expect("a block's length");

    let l_desc = Desc {
        stree: Some(&s.st.ltree),
        extra: &EXTRA_LBITS,
        base: N(LITERALS + 1),
        elems: N(L_CODES),
        max_length: N(MAX_BITS),
    };
    let l_max = build_tree(&mut s.h, &mut s.dyn_l, l_desc);
    let d_desc = Desc {
        stree: Some(&s.st.dtree),
        extra: &EXTRA_DBITS,
        base: N(0),
        elems: N(D_CODES),
        max_length: N(MAX_BITS),
    };
    let d_max = build_tree(&mut s.h, &mut s.dyn_d, d_desc);

    // build_bl_tree
    scan_tree(&mut s.bl, &mut s.dyn_l, l_max);
    scan_tree(&mut s.bl, &mut s.dyn_d, d_max);
    let bl_desc = Desc {
        stree: None,
        extra: &EXTRA_BLBITS,
        base: N(0),
        elems: N(BL_CODES),
        max_length: N(MAX_BL_BITS),
    };
    build_tree(&mut s.h, &mut s.bl, bl_desc);
    let mut max_blindex = BL_CODES - 1;
    while max_blindex >= 3 && s.bl[BL_ORDER[max_blindex]].dl.0 == 0 {
        max_blindex -= 1;
    }
    let tail = u64::from(ush(3 * (max_blindex + 1) + 5 + 5 + 4));
    s.h.opt_len.add(tail);

    let mut opt_lenb = s.h.opt_len.bytes();
    let static_lenb = s.h.static_len.bytes();
    if static_lenb <= opt_lenb {
        opt_lenb = static_lenb;
    }
    let as_stored = u64::try_from(stored).expect("a block's length") + 4;
    match start {
        Some(from) if as_stored <= opt_lenb => {
            s.out.send(last_bit, N(3));
            s.out.windup();
            let len = ush(stored & 0xffff);
            s.out.bytes.extend_from_slice(&len.to_le_bytes());
            s.out.bytes.extend_from_slice(&(!len).to_le_bytes());
            s.out
                .bytes
                .extend_from_slice(&s.window[from..from + stored]);
        }
        _ if static_lenb == opt_lenb => {
            s.out.send((1 << 1) + last_bit, N(3));
            compress_block(&mut s.out, &s.st, &s.syms, &s.st.ltree, &s.st.dtree);
        }
        _ => {
            s.out.send((2 << 1) + last_bit, N(3));
            // send_all_trees
            s.out.send(u64::from(ush(l_max + 1 - 257)), N(5));
            s.out.send(u64::from(ush(d_max)), N(5));
            s.out.send(u64::from(ush(max_blindex + 1 - 4)), N(4));
            for rank in BL_ORDER.iter().take(max_blindex + 1) {
                s.out.send(u64::from(s.bl[*rank].dl.0), N(3));
            }
            send_tree(&mut s.out, &s.bl, &s.dyn_l, l_max);
            send_tree(&mut s.out, &s.bl, &s.dyn_d, d_max);
            compress_block(&mut s.out, &s.st, &s.syms, &s.dyn_l, &s.dyn_d);
        }
    }
    init_block(s);
    if last == End::Last {
        s.out.windup();
    }
    s.block_start = i64::try_from(s.strstart.0).expect("a position");
}

/// Whether a block is the stream's last.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    More,
    Last,
}

/// zlib's tally of a literal.
fn tally_lit(s: &mut State<'_>, c: u8) -> bool {
    s.syms.push((0, c));
    s.dyn_l[usize::from(c)].fc.0 += 1;
    s.syms.len() == LIT_BUFSIZE - 1
}

/// zlib's tally of a length and a distance.
fn tally_dist(s: &mut State<'_>, dist: usize, len: usize) -> bool {
    let lc = u8::try_from(len).expect("a length less MIN_MATCH");
    s.syms.push((ush(dist), lc));
    let code = usize::from(s.st.length_code[len]);
    s.dyn_l[code + LITERALS + 1].fc.0 += 1;
    let d = s.st.d_code(dist - 1);
    s.dyn_d[d].fc.0 += 1;
    s.syms.len() == LIT_BUFSIZE - 1
}

/// `deflate_slow`.
fn deflate_slow(s: &mut State<'_>) {
    let mut available = Avail::No;
    loop {
        if s.lookahead.0 < MIN_LOOKAHEAD {
            fill_window(s);
            if s.lookahead.0 == 0 {
                break;
            }
        }
        let mut hash_head = 0;
        if s.lookahead.0 >= MIN_MATCH {
            hash_head = insert_string(s, s.strstart.0);
        }
        s.prev_length = s.match_length;
        let prev_match = s.match_start;
        s.match_length = N(MIN_MATCH - 1);
        if hash_head != 0 && s.prev_length.0 < MAX_LAZY && s.strstart.0 - hash_head <= MAX_DIST {
            s.match_length.0 = longest_match(s, hash_head);
            if s.match_length.0 <= 5
                && s.match_length.0 == MIN_MATCH
                && s.strstart.0.wrapping_sub(s.match_start.0) > TOO_FAR
            {
                s.match_length.0 = MIN_MATCH - 1;
            }
        }
        if s.prev_length.0 >= MIN_MATCH && s.match_length.0 <= s.prev_length.0 {
            let max_insert = s.strstart.0 + s.lookahead.0 - MIN_MATCH;
            let dist = (s.strstart.0 - 1).wrapping_sub(prev_match.0);
            let full = tally_dist(s, dist, s.prev_length.0 - MIN_MATCH);
            s.lookahead.0 -= s.prev_length.0 - 1;
            s.prev_length.0 -= 2;
            while s.prev_length.0 != 0 {
                s.strstart.0 += 1;
                if s.strstart.0 <= max_insert {
                    insert_string(s, s.strstart.0);
                }
                s.prev_length.0 -= 1;
            }
            available = Avail::No;
            s.match_length.0 = MIN_MATCH - 1;
            s.strstart.0 += 1;
            if full {
                flush_block(s, End::More);
            }
        } else if available == Avail::Yes {
            let c = s.window[s.strstart.0 - 1];
            if tally_lit(s, c) {
                flush_block(s, End::More);
            }
            s.strstart.0 += 1;
            s.lookahead.0 -= 1;
        } else {
            available = Avail::Yes;
            s.strstart.0 += 1;
            s.lookahead.0 -= 1;
        }
    }
    if available == Avail::Yes {
        let c = s.window[s.strstart.0 - 1];
        tally_lit(s, c);
    }
    flush_block(s, End::Last);
}

/// `match_available`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Avail {
    No,
    Yes,
}

/// `adler32`.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// `zlib.compress(data, 6)`.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut s = State {
        input: data,
        read: N(0),
        window: vec![0; WINDOW + MAX_MATCH],
        head: vec![0; HASH_SIZE],
        prev: vec![0; W_SIZE],
        ins_h: N(0),
        strstart: N(0),
        lookahead: N(0),
        match_start: N(0),
        match_length: N(MIN_MATCH - 1),
        prev_length: N(MIN_MATCH - 1),
        block_start: 0,
        syms: Vec::with_capacity(LIT_BUFSIZE),
        dyn_l: [Ct::default(); HEAP_SIZE],
        dyn_d: [Ct::default(); 2 * D_CODES + 1],
        bl: [Ct::default(); 2 * BL_CODES + 1],
        h: Heap {
            heap: [0; HEAP_SIZE],
            len: N(0),
            max: N(0),
            depth: [0; HEAP_SIZE],
            bl_count: [0; MAX_BITS + 1],
            opt_len: Bits(0),
            static_len: Bits(0),
        },
        out: Out {
            bytes: vec![0x78, 0x9c],
            acc: Bits(0),
            have: N(0),
        },
        st: Statics::new(),
    };
    init_block(&mut s);
    deflate_slow(&mut s);
    let mut out = s.out.bytes;
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every symbol a block can hold, as zlib counts: one short of the buffer.
    #[test]
    fn a_block_holds_one_symbol_fewer_than_its_buffer() {
        assert_eq!(LIT_BUFSIZE - 1, 16383);
    }

    #[test]
    fn the_static_trees_are_the_rfc_s() {
        let st = Statics::new();
        // RFC 1951 section 3.2.6: 144 gets the nine-bit code 110010000, sent
        // from its most significant bit, and 256 the seven-bit 0000000.
        assert_eq!(st.ltree[144].dl.0, 9);
        assert_eq!(st.ltree[144].fc.0, 0b0_0001_0011);
        assert_eq!(st.ltree[256].dl.0, 7);
        assert_eq!(st.ltree[256].fc.0, 0);
        assert_eq!(st.d_code(0), 0);
        assert_eq!(st.d_code(32767), 29);
        assert_eq!(st.length_code[255], 28);
    }
}
