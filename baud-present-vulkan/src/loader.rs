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
#[allow(unsafe_code)]
pub(crate) fn entry() -> Option<ash::Entry> {
    let handle = NAMES
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
