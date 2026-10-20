//! **The `wl_shm` pool: two buffers in one anonymous file.** The register's
//! D13 asks for a page-aligned file with a stride a multiple of 256 bytes,
//! so a compositor that imports shared memory as a dmabuf (KWin's udmabuf
//! path) does so without a copy. Two buffers, because the compositor holds
//! one while the client draws the next; `wl_buffer.release` says which is
//! free again.

use crate::sys;
use baud_wayland::{Arg, ObjectId, Opcode, Sender, protocol};
use std::io;
use std::os::fd::AsFd;

/// A pixel width or height, in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Px(pub u32);

pub const BUFFERS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Free,
    /// Attached and committed; the compositor has not released it.
    ByCompositor,
}

pub struct Pool {
    map: sys::Map,
    pool: ObjectId,
    buffers: [(ObjectId, Held); BUFFERS],
    width: Px,
    height: Px,
    /// Words a row, and `stride * 4` is a multiple of 256.
    stride: usize,
}

/// A stride in words such that the row's bytes are a multiple of 256.
fn stride_for(width: Px) -> usize {
    let bytes = usize::try_from(width.0).unwrap_or(0) * 4;
    bytes.div_ceil(256) * 256 / 4
}

impl Pool {
    /// Create the file, the pool and its two buffers of `width` by
    /// `height`, through `shm`.
    pub fn new(s: &mut Sender<'_>, shm: ObjectId, width: Px, height: Px) -> io::Result<Pool> {
        let stride = stride_for(width);
        let rows = usize::try_from(height.0).unwrap_or(0);
        let one = stride * rows * 4;
        let len = one * BUFFERS;
        let fd = sys::anonymous(sys::FileLen(len.max(4096)))?;
        let map = sys::Map::shared(&fd, sys::FileLen(len.max(4096)))?;
        let pool = s.new_id(baud_wayland::Interface::named("wl_shm_pool").expect("pinned"));
        let size = i32::try_from(len.max(4096))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pool size"))?;
        s.request(
            shm,
            Opcode(protocol::wl_shm::request::CREATE_POOL),
            &[Arg::NewId(pool), Arg::Fd(fd.as_fd()), Arg::Int(size)],
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        let mut buffers = [(ObjectId::NULL, Held::Free); BUFFERS];
        for (i, b) in buffers.iter_mut().enumerate() {
            let id = s.new_id(baud_wayland::Interface::named("wl_buffer").expect("pinned"));
            let offset = i32::try_from(i * one)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "offset"))?;
            let (w, h, st) = (
                i32::try_from(width.0).unwrap_or(0),
                i32::try_from(height.0).unwrap_or(0),
                i32::try_from(stride * 4).unwrap_or(0),
            );
            s.request(
                pool,
                Opcode(protocol::wl_shm_pool::request::CREATE_BUFFER),
                &[
                    Arg::NewId(id),
                    Arg::Int(offset),
                    Arg::Int(w),
                    Arg::Int(h),
                    Arg::Int(st),
                    Arg::Uint(protocol::wl_shm::format::ARGB8888),
                ],
            )
            .map_err(|e| io::Error::other(e.to_string()))?;
            *b = (id, Held::Free);
        }
        Ok(Pool {
            map,
            pool,
            buffers,
            width,
            height,
            stride,
        })
    }

    pub fn size(&self) -> (Px, Px) {
        (self.width, self.height)
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    /// A free buffer's id and its pixels, if the compositor has released one.
    pub fn free(&mut self) -> Option<(ObjectId, &mut [u32])> {
        let i = self.buffers.iter().position(|(_, h)| *h == Held::Free)?;
        let one = self.stride * usize::try_from(self.height.0).unwrap_or(0);
        let (id, _) = self.buffers[i];
        Some((id, &mut self.map.words()[i * one..(i + 1) * one]))
    }

    /// The buffer is attached and committed: the compositor holds it.
    pub fn committed(&mut self, buffer: ObjectId) {
        if let Some(b) = self.buffers.iter_mut().find(|(id, _)| *id == buffer) {
            b.1 = Held::ByCompositor;
        }
    }

    /// `wl_buffer.release`: the compositor is done with it.
    pub fn released(&mut self, buffer: ObjectId) -> bool {
        match self.buffers.iter_mut().find(|(id, _)| *id == buffer) {
            Some(b) => {
                b.1 = Held::Free;
                true
            }
            None => false,
        }
    }

    pub fn is_buffer(&self, id: ObjectId) -> bool {
        self.buffers.iter().any(|(b, _)| *b == id)
    }

    /// Destroy the buffers and the pool.
    pub fn destroy(self, s: &mut Sender<'_>) {
        for (id, _) in self.buffers {
            let _ = s.request(id, Opcode(protocol::wl_buffer::request::DESTROY), &[]);
        }
        let _ = s.request(
            self.pool,
            Opcode(protocol::wl_shm_pool::request::DESTROY),
            &[],
        );
    }
}
