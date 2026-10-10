//! Base64, decoded a byte at a time: `[[source: rfc4648-base64]]` section 4.
//!
//! **Incremental, because every caller's data spans reads.** An `OSC 52` body
//! of any size crosses a `read(2)` boundary wherever the kernel cut it, and a
//! kitty `t=d` payload is chunked by the protocol itself (`t-images.md`), so a
//! four-character quantum can arrive as one character in one read and three in
//! the next. The carry -- at most three sextets -- is the decoder's state and
//! nothing else, so the caller keeps one `Base64` across its reads and the
//! octets come out the same wherever the boundaries fell. One decoder for both
//! (note V, V11 and V19).
//!
//! **No buffer.** Each quantum's octets are handed back as they complete, and
//! where they go -- a bounded caller buffer, counted past its end -- is the
//! caller's: this crate allocates nothing.
//!
//! **What it accepts, against the text.**
//!
//! * The alphabet of Table 1 and `=`, and nothing else: section 3.3, "MUST
//!   reject the encoded data if it contains characters outside the base
//!   alphabet ... unless the specification referring to this document
//!   explicitly states otherwise". `[[source: ctlseqs]]` names the RFC for
//!   `OSC 52` and states nothing otherwise, so a line feed, a space or a pad
//!   before the end rejects the data, and so do excess pads, which section
//!   3.3 lets only such a specification ignore.
//! * Padding where section 4 puts it: `xx==` and `xxx=` end the data.
//! * **The unpadded final quanta `xx` and `xxx`.** Section 3.2's "MUST
//!   include appropriate pad characters" binds the encoder, and its reason --
//!   "when assumptions about the size of transported data cannot be made" --
//!   does not hold for a string whose end is its terminator. Rejecting would
//!   be allowed too; this is a choice, recorded where `finish` makes it.
//! * **Non-zero pad bits.** Section 3.5: decoders "MAY chose to reject an
//!   encoding if the pad bits have not been set to zero". This one does not.

/// The alphabet's values, indexed by byte: 0-63 for Table 1's characters,
/// [`NOT_BASE64`] for every other byte. Built once at compile time, so a
/// character is one load rather than a range test per class.
const VALUE: [u8; 256] = {
    let mut t = [NOT_BASE64; 256];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i < 64 {
        t[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    t
};

/// What [`VALUE`] holds for a byte outside the alphabet, `=` included.
const NOT_BASE64: u8 = 0xFF;

/// How many sextets of the quantum in progress are held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Zero,
    One,
    Two,
    Three,
}

/// Where the decoder stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Taking characters of the alphabet.
    Open,
    /// Two characters and one `=`: only the second `=` may follow.
    HalfPadded,
    /// The final quantum's pads arrived: the data has ended, and anything
    /// after them is a pad before the end.
    Ended,
    /// Rejected. Sticky: nothing that follows makes the data valid.
    Invalid,
}

/// The decoder's state: the sextets of the quantum in progress and where the
/// data stands. Eight bytes, `Copy`, so a caller keeps it in whatever state
/// struct outlives its reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Base64 {
    /// The held sextets, the first in the highest bits.
    acc: u32,
    held: Held,
    phase: Phase,
}

/// How many octets a quantum released.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OctetCount {
    Zero,
    One,
    Two,
    Three,
}

/// The octets one quantum decoded to: three for a whole quantum, one or two
/// for a short final one, none for empty data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Octets {
    bytes: [u8; 3],
    count: OctetCount,
}

impl Octets {
    /// No octets: what empty data, or a padded end, leaves to finish.
    pub const NONE: Octets = Octets {
        bytes: [0; 3],
        count: OctetCount::Zero,
    };

    /// The decoded octets.
    pub fn as_slice(&self) -> &[u8] {
        let n = match self.count {
            OctetCount::Zero => 0,
            OctetCount::One => 1,
            OctetCount::Two => 2,
            OctetCount::Three => 3,
        };
        &self.bytes[..n]
    }
}

/// What one character did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base64Step {
    /// Held: the quantum is not complete.
    Need,
    /// A quantum completed, and these are its octets.
    Octets(Octets),
    /// The data is rejected, now and for every character after.
    Invalid,
}

/// Why [`Base64::finish`] rejected the data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base64Error {
    /// A character outside the alphabet, or a pad where section 4 puts none,
    /// arrived before the end.
    NotBase64,
    /// The data ended on one sextet, which no octet can be made of, or on two
    /// characters and a single pad.
    Truncated,
}

impl Base64 {
    /// Nothing held, nothing seen.
    pub const NEW: Base64 = Base64 {
        acc: 0,
        held: Held::Zero,
        phase: Phase::Open,
    };

    /// One character of the encoded data.
    ///
    /// **Inlined into its caller, and that is measured.** Out of line, the
    /// linker laid its 448 bytes between `Paint::index` and
    /// `View::put_ascii_run`, the two functions `cat`'s ingest calls, and
    /// `ingest2` read `cat` 8% slower with none of the clipboard code run;
    /// base's code with this one function added, called once off the ingest
    /// path, read the same 8%. Inlined, the five recordings overlap base
    /// (`docs/record.md`, "OSC 52 writes reach the pasteboard", which also
    /// records the `multibench` cost this does not remove). Its one caller in
    /// the core is a cold function, which is where the code belongs.
    #[inline(always)]
    pub fn push(&mut self, b: u8) -> Base64Step {
        match self.phase {
            Phase::Open => {}
            Phase::HalfPadded if b == b'=' => {
                // `xx==`: one octet, from the first twelve bits.
                self.phase = Phase::Ended;
                return Base64Step::Octets(self.release(OctetCount::One));
            }
            Phase::HalfPadded | Phase::Ended | Phase::Invalid => return self.reject(),
        }
        let v = VALUE[b as usize];
        if v != NOT_BASE64 {
            let acc = (self.acc << 6) | u32::from(v);
            self.held = match self.held {
                Held::Zero => Held::One,
                Held::One => Held::Two,
                Held::Two => Held::Three,
                Held::Three => {
                    // The fourth sextet: a whole quantum, 24 bits, three octets.
                    self.acc = 0;
                    self.held = Held::Zero;
                    return Base64Step::Octets(octets(acc, OctetCount::Three));
                }
            };
            self.acc = acc;
            return Base64Step::Need;
        }
        match (b, self.held) {
            // `xx=` waits for its second pad; `xxx=` is two octets and the end.
            (b'=', Held::Two) => {
                self.phase = Phase::HalfPadded;
                Base64Step::Need
            }
            (b'=', Held::Three) => {
                self.phase = Phase::Ended;
                Base64Step::Octets(self.release(OctetCount::Two))
            }
            // A pad after none or one sextet, or any byte outside the
            // alphabet: section 3.3's rejection.
            _ => self.reject(),
        }
    }

    /// The data has ended: the octets of an unpadded final quantum, or why it
    /// is rejected.
    ///
    /// **`xx` and `xxx` decode as `xx==` and `xxx=` would.** The choice the
    /// module's comment states: an encoder must pad (section 3.2), and a
    /// decoder that knows where the data ends loses nothing by not requiring
    /// it. `x` alone is six bits and no octet, and `xx=` is half a padding;
    /// both are rejected.
    pub fn finish(self) -> Result<Octets, Base64Error> {
        match (self.phase, self.held) {
            (Phase::Invalid, _) => Err(Base64Error::NotBase64),
            (Phase::HalfPadded, _) | (Phase::Open, Held::One) => Err(Base64Error::Truncated),
            (Phase::Ended, _) | (Phase::Open, Held::Zero) => Ok(Octets::NONE),
            (Phase::Open, Held::Two) => {
                let mut d = self;
                Ok(d.release(OctetCount::One))
            }
            (Phase::Open, Held::Three) => {
                let mut d = self;
                Ok(d.release(OctetCount::Two))
            }
        }
    }

    /// The held sextets of a short final quantum as `count` octets, the
    /// quantum emptied. Shifted up to a whole 24 bits first, so the octets
    /// are the top of the word; the low bits two or three sextets leave are
    /// the pad bits, which section 3.5 lets this decoder ignore.
    fn release(&mut self, count: OctetCount) -> Octets {
        let word = match self.held {
            Held::Zero => 0,
            Held::One => self.acc << 18,
            Held::Two => self.acc << 12,
            Held::Three => self.acc << 6,
        };
        self.acc = 0;
        self.held = Held::Zero;
        octets(word, count)
    }

    fn reject(&mut self) -> Base64Step {
        self.phase = Phase::Invalid;
        Base64Step::Invalid
    }
}

/// The top `count` octets of a 24-bit word.
fn octets(word: u32, count: OctetCount) -> Octets {
    Octets {
        bytes: [(word >> 16) as u8, (word >> 8) as u8, word as u8],
        count,
    }
}
