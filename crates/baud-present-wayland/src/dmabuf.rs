//! **The GPU path's two protocols: buffers as dmabufs, and explicit
//! synchronisation.** `linux-dmabuf-v1` says which formats and modifiers
//! the compositor can import, and makes a `wl_buffer` of a dmabuf's planes;
//! `linux-drm-syncobj-v1` says, per commit, the timeline point the
//! compositor waits on before it reads the buffer and the point it signals
//! once it no longer needs it (`[[source: wp-linux-dmabuf-v1]]`,
//! `[[source: wp-linux-drm-syncobj-v1]]`). The renderer allocates and
//! signals; this module only speaks the protocol, and every piece is driven
//! by the fake compositor's tests.
//!
//! **The formats arrive as feedback, gathered and then published whole.**
//! "The parameters are sent once when this object is created and whenever
//! they change", "The done event is always sent once after all parameters
//! have been sent", and "When a single parameter changes, all parameters
//! are re-sent": [`Gather`] collects a burst and [`Gather::done`] hands it
//! over as one [`Feedback`], so a reader never sees half of one. The
//! format table is a file of "consecutive format + modifier pairs. Each
//! pair is 16 bytes wide", mapped "in read-only private mode", and stays in
//! force until another is sent: "the compositor is free not to send another
//! format_table event". A tranche names its formats by "16-bit unsigned
//! integer" indices into it, "in native endianness".

use crate::shm::Px;
use crate::sys;
use baud_wayland::{Arg, ObjectId, Opcode, Sender, protocol};
use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};

/// A DRM fourcc format code: four ASCII bytes, the first lowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fourcc(pub u32);

impl Fourcc {
    /// `DRM_FORMAT_ARGB8888`, `AR24`: what the shm path's buffers hold.
    pub const ARGB8888: Fourcc = Fourcc(u32::from_le_bytes(*b"AR24"));
    /// `DRM_FORMAT_XRGB8888`, `XR24`: the same with the alpha ignored.
    pub const XRGB8888: Fourcc = Fourcc(u32::from_le_bytes(*b"XR24"));
}

/// A DRM format modifier: "an opaque token" for a layout -- tiling,
/// compression -- of the base format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Modifier(pub u64);

impl Modifier {
    /// `DRM_FORMAT_MOD_LINEAR`: rows one after another.
    pub const LINEAR: Modifier = Modifier(0);
    /// `DRM_FORMAT_MOD_INVALID`: "the server can support the format with
    /// an implicit modifier", the layout "derived from the dmabuf".
    pub const INVALID: Modifier = Modifier(0x00ff_ffff_ffff_ffff);

    /// The wire's two words, high first.
    pub fn halves(self) -> [Word; 2] {
        Wide(self.0).split()
    }
}

/// A format and a modifier: one entry of the format table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Format {
    pub fourcc: Fourcc,
    pub modifier: Modifier,
}

/// A device, as a `dev_t`: "Clients cannot check two devices for equality
/// by comparing the dev_t value", so it is something to open, not a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device(pub u64);

impl Device {
    /// The array a device event carries: one `dev_t`, eight bytes on the
    /// 64-bit targets, in the machine's order. Any other length names no
    /// device.
    pub fn of(bytes: &[u8]) -> Option<Device> {
        <[u8; 8]>::try_from(bytes)
            .ok()
            .map(|b| Device(u64::from_ne_bytes(b)))
    }
}

/// `tranche_flags`: the `scanout` bit, "a hint that direct scan-out may be
/// attempted", and from version 6 `sampling`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrancheFlags(pub u32);

impl TrancheFlags {
    pub const SCANOUT: TrancheFlags =
        TrancheFlags(protocol::zwp_linux_dmabuf_feedback_v1::tranche_flags::SCANOUT);

    pub fn has(self, f: TrancheFlags) -> bool {
        self.0 & f.0 == f.0
    }
}

/// One tranche: a target device, its flags and its formats, all of one
/// preference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tranche {
    pub device: Option<Device>,
    pub flags: TrancheFlags,
    pub formats: Vec<Format>,
}

/// One whole burst of feedback: the main device (below version 6) and the
/// tranches "in descending order of preference".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feedback {
    pub main: Option<Device>,
    pub tranches: Vec<Tranche>,
    /// Indices a tranche named that the table does not hold: skipped, and
    /// counted so a short table is seen rather than read as fewer formats.
    pub unknown_indices: Count,
}

/// A number of things counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Count(pub usize);

impl Feedback {
    /// Whether any tranche offers `f`: "Starting from version 4, the
    /// invalid_format protocol error is sent if the format + modifier pair
    /// was not advertised as supported", so nothing else is imported.
    pub fn offers(&self, f: Format) -> bool {
        self.tranches.iter().any(|t| t.formats.contains(&f))
    }

    /// The modifiers offered for `fourcc`, in the tranches' order of
    /// preference, each once.
    pub fn modifiers(&self, fourcc: Fourcc) -> Vec<Modifier> {
        let mut out = Vec::new();
        for f in self.tranches.iter().flat_map(|t| &t.formats) {
            if f.fourcc == fourcc && !out.contains(&f.modifier) {
                out.push(f.modifier);
            }
        }
        out
    }
}

/// The table's bytes an entry takes: "Each pair is 16 bytes wide".
const ENTRY: usize = 16;

/// **A feedback burst as it arrives**, published by [`Gather::done`].
#[derive(Debug, Default)]
pub struct Gather {
    /// The last format table, kept across bursts.
    table: Vec<Format>,
    main: Option<Device>,
    tranches: Vec<Tranche>,
    tranche: Tranche,
    unknown: Count,
}

impl Gather {
    /// `format_table`: map `fd`'s `len` bytes read-only and private and
    /// read every whole entry. A table that cannot be mapped leaves none,
    /// so every index after it is unknown rather than a stale table's.
    pub fn table(&mut self, fd: &OwnedFd, len: sys::FileLen) -> io::Result<()> {
        self.table.clear();
        if len.0 == 0 {
            return Ok(());
        }
        let map = sys::ReadMap::private(fd, len)?;
        let (entries, _) = map.bytes().as_chunks::<ENTRY>();
        self.table = entries
            .iter()
            .map(|e| Format {
                fourcc: Fourcc(u32::from_ne_bytes([e[0], e[1], e[2], e[3]])),
                modifier: Modifier(u64::from_ne_bytes([
                    e[8], e[9], e[10], e[11], e[12], e[13], e[14], e[15],
                ])),
            })
            .collect();
        Ok(())
    }

    /// The entries the last table held.
    pub fn table_len(&self) -> Count {
        Count(self.table.len())
    }

    pub fn main_device(&mut self, bytes: &[u8]) {
        self.main = Device::of(bytes);
    }

    pub fn target_device(&mut self, bytes: &[u8]) {
        self.tranche.device = Device::of(bytes);
    }

    pub fn flags(&mut self, f: TrancheFlags) {
        self.tranche.flags = f;
    }

    /// `tranche_formats`: "one or more" a tranche, each an array of
    /// indices into the last table.
    pub fn formats(&mut self, indices: &[u8]) {
        let (pairs, _) = indices.as_chunks::<2>();
        for pair in pairs {
            let i = usize::from(u16::from_ne_bytes(*pair));
            match self.table.get(i) {
                Some(f) => self.tranche.formats.push(*f),
                None => self.unknown.0 += 1,
            }
        }
    }

    pub fn tranche_done(&mut self) {
        self.tranches.push(core::mem::take(&mut self.tranche));
    }

    /// `done`: the burst, whole. The table stays for the next.
    pub fn done(&mut self) -> Feedback {
        self.tranche = Tranche::default();
        Feedback {
            main: self.main.take(),
            tranches: core::mem::take(&mut self.tranches),
            unknown_indices: core::mem::take(&mut self.unknown),
        }
    }
}

/// A 32-bit word of the wire: half a 64-bit value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word(pub u32);

/// A 64-bit value the wire carries as two words: a modifier, a point.
#[derive(Clone, Copy)]
struct Wide(u64);

impl Wide {
    /// The high word, then the low.
    fn split(self) -> [Word; 2] {
        let hi = u32::try_from(self.0 >> 32).unwrap_or(u32::MAX);
        let lo = u32::try_from(self.0 & u64::from(u32::MAX)).unwrap_or(u32::MAX);
        [Word(hi), Word(lo)]
    }
}

/// A byte offset or a row's bytes, as `add` carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bytes(pub u32);

/// One plane of a dmabuf: its file, where the plane starts in it and its
/// row pitch.
#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: Bytes,
    pub stride: Bytes,
}

/// The planes `zwp_linux_buffer_params_v1` takes at most: `plane_idx` is
/// "too large" past the fourth.
pub const MAX_PLANES: usize = 4;

/// A dmabuf to make a `wl_buffer` of.
#[derive(Clone, Copy, Debug)]
pub struct Dmabuf<'a> {
    pub width: Px,
    pub height: Px,
    pub format: Format,
    pub planes: &'a [Plane<'a>],
}

/// A buffer asked of the compositor, its answer to come
/// ([`crate::Window::answer`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params(pub ObjectId);

/// **The compositor's answer to a buffer asked**: none yet, the
/// `wl_buffer` it made, or a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Waiting,
    Buffer(ObjectId),
    /// `failed`: "one of the dmabuf constraints has not been fulfilled".
    Refused,
}

/// Whether a dmabuf was asked for as a `wl_buffer`, and if not, why: each
/// refusal is a request that would have been a fatal protocol error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Imported {
    /// Asked: the compositor answers `created` or `failed`.
    Asked(Params),
    /// The compositor offers no `zwp_linux_dmabuf_v1` at version 4 or
    /// later, or has sent no feedback yet.
    NoFormats,
    /// The pair is not in the feedback: `invalid_format`.
    NotOffered,
    /// No plane, or more than [`MAX_PLANES`]: `incomplete`, `plane_idx`.
    Planes,
    /// A zero side: `invalid_dimensions`.
    Empty,
}

/// **A `wl_buffer` of `d` asked for**, through `create_params`, an `add` for
/// each plane and `create`, whose answer is an event: "the wl_buffer is not
/// created immediately but returned via the 'created' event if the dmabuf
/// sharing succeeds", and "the sharing may fail at runtime for reasons a
/// client cannot predict, in which case the 'failed' event is triggered".
/// Not `create_immed`, whose failure is a fatal error or "an invalid
/// wl_buffer" whose use "is defined by the compositor
/// implementation" (`[[source: wp-linux-dmabuf-v1]]`): a refusal
/// here is seen, and the window answers it ([`crate::Window::answer`]). Every plane carries the one modifier, "Starting from
/// version 5, the invalid_format protocol error is sent if all planes
/// don't use the same modifier".
pub fn import(
    s: &mut Sender<'_>,
    manager: ObjectId,
    offered: &Feedback,
    d: &Dmabuf<'_>,
) -> Imported {
    if d.planes.is_empty() || d.planes.len() > MAX_PLANES {
        return Imported::Planes;
    }
    if d.width.0 == 0 || d.height.0 == 0 {
        return Imported::Empty;
    }
    if !offered.offers(d.format) {
        return Imported::NotOffered;
    }
    use protocol::zwp_linux_buffer_params_v1::request as params;
    let p = s.new_id(&protocol::zwp_linux_buffer_params_v1::INTERFACE);
    let _ = s.request(
        manager,
        Opcode(protocol::zwp_linux_dmabuf_v1::request::CREATE_PARAMS),
        &[Arg::NewId(p)],
    );
    let [Word(hi), Word(lo)] = d.format.modifier.halves();
    for (i, plane) in (0u32..).zip(d.planes) {
        let _ = s.request(
            p,
            Opcode(params::ADD),
            &[
                Arg::Fd(plane.fd),
                Arg::Uint(i),
                Arg::Uint(plane.offset.0),
                Arg::Uint(plane.stride.0),
                Arg::Uint(hi),
                Arg::Uint(lo),
            ],
        );
    }
    let side = |p: Px| Arg::Int(i32::try_from(p.0).unwrap_or(i32::MAX));
    let _ = s.request(
        p,
        Opcode(params::CREATE),
        &[
            side(d.width),
            side(d.height),
            Arg::Uint(d.format.fourcc.0),
            Arg::Uint(0),
        ],
    );
    Imported::Asked(Params(p))
}

/// A DRM syncobj timeline the compositor has imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeline(pub ObjectId);

/// A point on a timeline: "The 64-bit unsigned value combined from
/// point_hi and point_lo is the point value."
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point(pub u64);

/// **The two points a commit's buffer is synchronised by**: the one the
/// compositor waits on before it samples the buffer, and the one it
/// signals once done with it. "it is strongly recommended that each buffer
/// should use a separate timeline for its release points", which is the
/// renderer's to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Points {
    pub acquire: (Timeline, Point),
    pub release: (Timeline, Point),
}

impl Points {
    /// "If at surface commit time the acquire and release DRM syncobj
    /// timelines are identical, the acquire point value must be strictly
    /// less than the release point value, or else the conflicting_points
    /// protocol error is raised."
    pub fn conflict(&self) -> bool {
        self.acquire.0 == self.release.0 && self.acquire.1 >= self.release.1
    }
}

/// **Import `fd`, a DRM syncobj, as a timeline**, through the manager.
pub fn import_timeline(s: &mut Sender<'_>, manager: ObjectId, fd: BorrowedFd<'_>) -> Timeline {
    let id = s.new_id(&protocol::wp_linux_drm_syncobj_timeline_v1::INTERFACE);
    let _ = s.request(
        manager,
        Opcode(protocol::wp_linux_drm_syncobj_manager_v1::request::IMPORT_TIMELINE),
        &[Arg::NewId(id), Arg::Fd(fd)],
    );
    Timeline(id)
}

/// Destroy a timeline: points set on it before the last commit stand.
pub fn destroy_timeline(s: &mut Sender<'_>, t: Timeline) {
    let _ = s.request(
        t.0,
        Opcode(protocol::wp_linux_drm_syncobj_timeline_v1::request::DESTROY),
        &[],
    );
}

/// Set both points on `sync`, the surface's syncobj object, for the next
/// commit: "double-buffered state, and will be applied on the next
/// wl_surface.commit".
pub(crate) fn set_points(s: &mut Sender<'_>, sync: ObjectId, p: Points) {
    use protocol::wp_linux_drm_syncobj_surface_v1::request as req;
    for (op, (t, at)) in [
        (req::SET_ACQUIRE_POINT, p.acquire),
        (req::SET_RELEASE_POINT, p.release),
    ] {
        let [Word(hi), Word(lo)] = Wide(at.0).split();
        let _ = s.request(
            sync,
            Opcode(op),
            &[Arg::Object(t.0), Arg::Uint(hi), Arg::Uint(lo)],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn table_file(entries: &[Format]) -> (OwnedFd, sys::FileLen) {
        let mut bytes = Vec::new();
        for e in entries {
            bytes.extend_from_slice(&e.fourcc.0.to_ne_bytes());
            bytes.extend_from_slice(&[0; 4]);
            bytes.extend_from_slice(&e.modifier.0.to_ne_bytes());
        }
        let fd = sys::anonymous(sys::FileLen(bytes.len())).unwrap();
        let mut f = std::fs::File::from(fd);
        f.write_all(&bytes).unwrap();
        (OwnedFd::from(f), sys::FileLen(bytes.len()))
    }

    fn indices(of: &[u16]) -> Vec<u8> {
        of.iter().flat_map(|i| i.to_ne_bytes()).collect()
    }

    const TILED: Modifier = Modifier(0x0200_0000_1234_5678);

    /// **A burst is published whole at `done`, and the table outlives
    /// it**: the table's sixteen-byte entries read in the machine's order,
    /// two tranches in the order sent with their devices and flags, an
    /// index past the table counted rather than read, and a device array
    /// of the wrong length naming none. A second burst with no table of
    /// its own reads the first's, and starts with no tranche left over.
    #[test]
    fn a_burst_is_published_whole_and_the_table_outlives_it() {
        let argb_linear = Format {
            fourcc: Fourcc::ARGB8888,
            modifier: Modifier::LINEAR,
        };
        let argb_tiled = Format {
            fourcc: Fourcc::ARGB8888,
            modifier: TILED,
        };
        let xrgb_implicit = Format {
            fourcc: Fourcc::XRGB8888,
            modifier: Modifier::INVALID,
        };
        let (fd, len) = table_file(&[argb_linear, argb_tiled, xrgb_implicit]);
        let mut g = Gather::default();
        g.table(&fd, len).unwrap();
        assert_eq!(g.table_len(), Count(3));
        let dev = 0xe200_u64.to_ne_bytes();
        g.main_device(&dev);
        g.target_device(&dev);
        g.flags(TrancheFlags::SCANOUT);
        g.formats(&indices(&[1]));
        g.formats(&indices(&[0, 9]));
        g.tranche_done();
        g.target_device(&[1, 2, 3]);
        g.flags(TrancheFlags(0));
        g.formats(&indices(&[2, 0]));
        g.tranche_done();
        let f = g.done();
        assert_eq!(f.main, Some(Device(0xe200)));
        assert_eq!(f.tranches.len(), 2);
        assert_eq!(f.tranches[0].device, Some(Device(0xe200)));
        assert!(f.tranches[0].flags.has(TrancheFlags::SCANOUT));
        assert_eq!(f.tranches[0].formats, vec![argb_tiled, argb_linear]);
        assert_eq!(f.tranches[1].device, None, "three bytes are no dev_t");
        assert!(!f.tranches[1].flags.has(TrancheFlags::SCANOUT));
        assert_eq!(f.unknown_indices, Count(1));
        assert_eq!(
            f.modifiers(Fourcc::ARGB8888),
            vec![TILED, Modifier::LINEAR],
            "in preference order, each once"
        );
        assert!(f.offers(xrgb_implicit));
        assert!(!f.offers(Format {
            fourcc: Fourcc::XRGB8888,
            modifier: Modifier::LINEAR
        }));

        g.target_device(&dev);
        g.formats(&indices(&[2]));
        g.tranche_done();
        let again = g.done();
        assert_eq!(again.main, None, "a burst's main device is its own");
        assert_eq!(again.tranches.len(), 1);
        assert_eq!(again.tranches[0].formats, vec![xrgb_implicit]);
        assert_eq!(again.unknown_indices, Count(0));
    }

    /// The fourcc codes are their four letters, first lowest, and a
    /// 64-bit value splits high word first.
    #[test]
    fn the_codes_are_their_letters_and_values_split_high_first() {
        assert_eq!(Fourcc::ARGB8888.0, 0x3432_5241);
        assert_eq!(Fourcc::XRGB8888.0, 0x3432_5258);
        assert_eq!(TILED.halves(), [Word(0x0200_0000), Word(0x1234_5678)]);
    }

    /// **Conflicting points are those on one timeline whose acquire is not
    /// before its release**; on two timelines any pair stands.
    #[test]
    fn points_conflict_only_on_one_timeline_out_of_order() {
        let (a, b) = (Timeline(ObjectId(7)), Timeline(ObjectId(8)));
        let p = |x, y, t2| Points {
            acquire: (a, Point(x)),
            release: (t2, Point(y)),
        };
        assert!(!p(1, 2, a).conflict());
        assert!(p(2, 2, a).conflict());
        assert!(p(3, 2, a).conflict());
        assert!(!p(3, 2, b).conflict());
    }
}
