//! **The system's Vulkan loader, opened at run time**: `libvulkan.so.1`
//! by `dlopen`, and its one entry point, `vkGetInstanceProcAddr`, by
//! `dlsym`, from which `ash` loads the rest. A system without the loader
//! runs on shared memory rather than failing to start. The two calls are
//! declared as the externs they are, the way the Wayland crates declare
//! theirs, and are the crate's third place `unsafe` is allowed.
//!
//! The library stays open for the process's life: an instance or a device
//! made through it may outlive any one owner.

use ash::vk;
use core::ffi::{CStr, c_char, c_void};

#[allow(unsafe_code)]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: Open) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// `dlopen`'s flags, an `int`.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Open(i32);

/// `RTLD_NOW`: every symbol bound at the open, so a broken loader fails
/// here and not at a later call.
const RTLD_NOW: Open = Open(2);

/// The loader's names, the versioned one first, as the Vulkan loader's
/// own documentation tells an application to open it.
const NAMES: [&CStr; 2] = [c"libvulkan.so.1", c"libvulkan.so"];

/// **The loader's entry**, or `None` where no loader opens or it lacks
/// `vkGetInstanceProcAddr`.
pub(crate) fn entry() -> Option<ash::Entry> {
    entry_from(&NAMES)
}

/// **The entry of the first of `names` that opens**, or `None` where none
/// does or the one that opens lacks `vkGetInstanceProcAddr`.
#[allow(unsafe_code)]
fn entry_from(names: &[&CStr]) -> Option<ash::Entry> {
    let handle = names
        .iter()
        // SAFETY: a NUL-terminated name; dlopen runs the library's
        // initialisers, which the Vulkan loader's are.
        .map(|name| unsafe { dlopen(name.as_ptr(), RTLD_NOW) })
        .find(|h| !h.is_null())?;
    // SAFETY: an open handle and a NUL-terminated name.
    let f = unsafe { dlsym(handle, c"vkGetInstanceProcAddr".as_ptr()) };
    if f.is_null() {
        return None;
    }
    // SAFETY: the loader exports `vkGetInstanceProcAddr` with this
    // signature, and the library is never closed, so the pointer stays valid.
    let get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr = unsafe { core::mem::transmute(f) };
    // SAFETY: as above: Vulkan 1.0's entry point, valid for the process.
    Some(unsafe {
        ash::Entry::from_static_fn(ash::StaticFn {
            get_instance_proc_addr,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A loader that is missing, or a library without the entry point,
    /// is no loader**, which the arm takes as the CPU's: a name nothing
    /// answers, and the C library, which opens and lacks
    /// `vkGetInstanceProcAddr`. The control: the system's loader opens.
    #[test]
    fn a_missing_loader_or_one_without_its_entry_is_none() {
        assert!(entry_from(&[c"libbaud-no-such-loader.so.1"]).is_none());
        assert!(entry_from(&[c"libc.so.6"]).is_none(), "no entry point");
        assert!(
            entry_from(&NAMES).is_some(),
            "the control: the loader opens"
        );
    }
}
