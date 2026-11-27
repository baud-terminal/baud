//! kitty's interpreter and its `Screen`, reached at run time.
//!
//! Nothing here is linked. `Kitty::load` hands the extension to the loader,
//! which brings the libpython the extension was built against, and every call
//! after that goes through a pointer `dlsym` returned. Only the functions of
//! CPython's stable ABI are used, and no CPython struct is laid out here but
//! `PyMethodDef`, which that ABI fixes. No Python source is run: there is no
//! `PyRun_*`, and `Callbacks` is built by calling `type()`.

use crate::Error;
use crate::cycle::{self, Cycle};
use crate::locate::Location;
use std::ffi::{CStr, CString, OsStr, OsString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

type Obj = *mut c_void;

const METH_VARARGS_KEYWORDS: c_int = 0x0001 | 0x0002;
const METH_O: c_int = 0x0008;
const PYBUF_READ: c_int = 0x100;

/// CPython's `PyMethodDef`, as the stable ABI lays it out.
#[repr(C)]
struct MethodDef {
    name: *const c_char,
    method: *const c_void,
    flags: c_int,
    doc: *const c_char,
}

/// The dynamic loader's last refusal.
fn dlerror() -> String {
    // SAFETY: dlerror returns null or a NUL-terminated string valid until the
    // next loader call, and it is copied before that.
    unsafe {
        let e = libc::dlerror();
        if e.is_null() {
            return "unknown".to_string();
        }
        CStr::from_ptr(e).to_string_lossy().into_owned()
    }
}

/// A library the loader has opened. It stays open for the life of the process.
#[derive(Debug)]
pub struct Library {
    handle: *mut c_void,
    path: PathBuf,
}

/// Load a library so that its symbols are visible to what is loaded after it.
pub fn open_library(library: &Path) -> Result<Library, Error> {
    let c = CString::new(library.as_os_str().as_encoded_bytes()).map_err(|_| Error::Load {
        library: library.to_path_buf(),
        reason: "a NUL in the path".to_string(),
    })?;
    // SAFETY: `c` is NUL-terminated and outlives the call.
    let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    if h.is_null() {
        return Err(Error::Load {
            library: library.to_path_buf(),
            reason: dlerror(),
        });
    }
    Ok(Library {
        handle: h,
        path: library.to_path_buf(),
    })
}

macro_rules! api {
    ($($field:ident : $symbol:literal : $ty:ty,)*) => {
        /// The CPython functions this harness calls.
        pub struct Api { $($field: $ty,)* }

        impl Api {
            /// Every symbol, from what `library` brought with it. A library that
            /// lacks one is not a Python.
            pub fn load(library: &Library) -> Result<Api, Error> {
                Ok(Api { $($field: {
                    let name = concat!($symbol, "\0");
                    // SAFETY: `name` is NUL-terminated, and a found symbol is
                    // that function of CPython's stable ABI with this signature.
                    unsafe {
                        let p = libc::dlsym(library.handle, name.as_ptr().cast());
                        if p.is_null() {
                            return Err(Error::NotPython {
                                library: library.path.clone(),
                                symbol: $symbol.to_string(),
                            });
                        }
                        std::mem::transmute::<*mut c_void, $ty>(p)
                    }
                },)* })
            }
        }
    };
}

api! {
    initialize_ex: "Py_InitializeEx": unsafe extern "C" fn(c_int),
    get_version: "Py_GetVersion": unsafe extern "C" fn() -> *const c_char,
    inc_ref: "Py_IncRef": unsafe extern "C" fn(Obj),
    dec_ref: "Py_DecRef": unsafe extern "C" fn(Obj),
    err_print: "PyErr_Print": unsafe extern "C" fn(),
    err_occurred: "PyErr_Occurred": unsafe extern "C" fn() -> Obj,
    import_module: "PyImport_ImportModule": unsafe extern "C" fn(*const c_char) -> Obj,
    get_attr: "PyObject_GetAttrString": unsafe extern "C" fn(Obj, *const c_char) -> Obj,
    set_attr: "PyObject_SetAttrString": unsafe extern "C" fn(Obj, *const c_char, Obj) -> c_int,
    call_function: "PyObject_CallFunction": unsafe extern "C" fn(Obj, *const c_char, ...) -> Obj,
    call_method: "PyObject_CallMethod": unsafe extern "C" fn(Obj, *const c_char, *const c_char, ...) -> Obj,
    call_obj_args: "PyObject_CallFunctionObjArgs": unsafe extern "C" fn(Obj, ...) -> Obj,
    build_value: "Py_BuildValue": unsafe extern "C" fn(*const c_char, ...) -> Obj,
    memoryview_from_memory: "PyMemoryView_FromMemory": unsafe extern "C" fn(*mut c_char, isize, c_int) -> Obj,
    long_as_ssize_t: "PyLong_AsSsize_t": unsafe extern "C" fn(Obj) -> isize,
    long_from_ssize_t: "PyLong_FromSsize_t": unsafe extern "C" fn(isize) -> Obj,
    object_str: "PyObject_Str": unsafe extern "C" fn(Obj) -> Obj,
    unicode_as_utf8: "PyUnicode_AsUTF8": unsafe extern "C" fn(Obj) -> *const c_char,
    cfunction_new: "PyCFunction_NewEx": unsafe extern "C" fn(*const MethodDef, Obj, Obj) -> Obj,
}

static API: OnceLock<Api> = OnceLock::new();
static NOOP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static CALLBACKS_TAKEN: AtomicU64 = AtomicU64::new(0);

/// A callable that takes anything and returns `None`.
unsafe extern "C" fn noop(_module: Obj, _args: Obj, _kwargs: Obj) -> Obj {
    let api = API.get().expect("loaded before any callback can arrive");
    (api.build_value)(c"".as_ptr())
}

/// `Callbacks.__getattr__`: whatever the parser asks for is `noop`. It is a
/// builtin on the class, so the instance is not bound and `name` is its argument.
unsafe extern "C" fn any_attribute(_module: Obj, _name: Obj) -> Obj {
    let api = API.get().expect("loaded before any callback can arrive");
    CALLBACKS_TAKEN.fetch_add(1, Ordering::Relaxed);
    let f = NOOP.load(Ordering::Relaxed);
    (api.inc_ref)(f);
    f
}

/// The `PYTHONHOME` that keeps the interpreter on the standard library of the
/// libpython in hand, from that library's path and its version string. Without
/// it, a `python3` of the same minor version earlier on `PATH` would be taken
/// for the prefix.
pub fn python_home(libpython: &Path, version: &str) -> Option<PathBuf> {
    let mut parts = version.split('.');
    let minor = format!("python{}.{}", parts.next()?, parts.next()?);
    let dir = libpython.parent()?;
    let prefix = if dir.file_name() == Some(OsStr::new("lib")) {
        dir.parent()?
    } else {
        dir
    };
    prefix
        .join("lib")
        .join(minor)
        .join("os.py")
        .is_file()
        .then(|| prefix.to_path_buf())
}

/// The environment the interpreter starts with: no `PYTHON*` of the caller's,
/// the kitty's package root as the one addition to the path, nothing written
/// into that kitty, no site-packages of the user's.
pub fn interpreter_env(
    inherited: impl IntoIterator<Item = OsString>,
    package_root: &Path,
    home: Option<&Path>,
) -> (Vec<OsString>, Vec<(&'static str, OsString)>) {
    let remove = inherited
        .into_iter()
        .filter(|k| k.as_encoded_bytes().starts_with(b"PYTHON"))
        .collect();
    let mut set = vec![
        ("PYTHONPATH", package_root.as_os_str().to_os_string()),
        ("PYTHONDONTWRITEBYTECODE", OsString::from("1")),
        ("PYTHONNOUSERSITE", OsString::from("1")),
    ];
    if let Some(h) = home {
        set.push(("PYTHONHOME", h.as_os_str().to_os_string()));
    }
    (remove, set)
}

/// A strong reference, released on drop.
pub struct Ref {
    api: &'static Api,
    p: Obj,
}

impl Drop for Ref {
    fn drop(&mut self) {
        // SAFETY: `p` is a strong reference this owns.
        unsafe { (self.api.dec_ref)(self.p) }
    }
}

/// kitty's interpreter, up, with its extension imported. It stays up until the
/// process exits. Not `Send`: the thread that loads it holds the GIL for good.
pub struct Kitty {
    api: &'static Api,
    screen_type: Ref,
    callbacks_class: Ref,
    _not_send: std::marker::PhantomData<*mut ()>,
}

impl Kitty {
    /// Load `location`'s extension and bring its interpreter up. `libpython` is
    /// for a kitty whose extension does not itself link one.
    pub fn load(location: &Location, libpython: Option<&Path>) -> Result<Kitty, Error> {
        let named = libpython.map(open_library).transpose()?;
        let extension = open_library(&location.extension)?;
        let api = Api::load(named.as_ref().unwrap_or(&extension))?;
        let api: &'static Api = API.get_or_init(|| api);

        // SAFETY: the interpreter is not up yet, Py_GetVersion is callable before
        // that, and everything after Py_InitializeEx runs on this thread, which
        // holds the GIL from then on.
        unsafe {
            let version = CStr::from_ptr((api.get_version)()).to_string_lossy();
            let home = python_library(api).and_then(|l| python_home(&l, &version));
            let inherited = std::env::vars_os().map(|(k, _)| k);
            let (remove, set) = interpreter_env(inherited, &location.package_root, home.as_deref());
            for k in remove {
                std::env::remove_var(k);
            }
            for (k, v) in set {
                std::env::set_var(k, v);
            }
            (api.initialize_ex)(0);

            let module = checked(
                api,
                (api.import_module)(c"kitty.fast_data_types".as_ptr()),
                "import kitty.fast_data_types",
            )?;
            let screen_type = checked(
                api,
                (api.get_attr)(module.p, c"Screen".as_ptr()),
                "fast_data_types.Screen",
            )?;
            let callbacks_class = callbacks_class(api)?;
            Ok(Kitty {
                api,
                screen_type,
                callbacks_class,
                _not_send: std::marker::PhantomData,
            })
        }
    }

    /// The interpreter's own version line.
    pub fn python_version(&self) -> String {
        // SAFETY: a static NUL-terminated string.
        unsafe { CStr::from_ptr((self.api.get_version)()) }
            .to_string_lossy()
            .into_owned()
    }

    /// How many attributes the parser has asked `Callbacks` for, in this process.
    pub fn callbacks_taken(&self) -> u64 {
        CALLBACKS_TAKEN.load(Ordering::Relaxed)
    }

    /// A Python integer, which is no write buffer: for showing a broken cycle
    /// is reported.
    pub fn integer(&self, v: isize) -> Result<Ref, Error> {
        // SAFETY: the interpreter is up on this thread.
        unsafe {
            checked(
                self.api,
                (self.api.long_from_ssize_t)(v),
                "PyLong_FromSsize_t",
            )
        }
    }

    /// A new `Screen`: `lines` by `columns`, `scrollback` lines of history, cells
    /// of 10 by 20, window id 0, and one `Callbacks` as both callback arguments.
    pub fn screen(
        &self,
        lines: c_int,
        columns: c_int,
        scrollback: c_int,
    ) -> Result<Screen<'_>, Error> {
        let api = self.api;
        // SAFETY: the interpreter is up on this thread; the format matches the
        // arguments, and each returned reference is checked before use.
        unsafe {
            let cb = checked(
                api,
                (api.call_obj_args)(self.callbacks_class.p, null_mut::<c_void>()),
                "Callbacks()",
            )?;
            let screen = checked(
                api,
                (api.call_function)(
                    self.screen_type.p,
                    c"OiiiiiiO".as_ptr(),
                    cb.p,
                    lines,
                    columns,
                    scrollback,
                    10 as c_int,
                    20 as c_int,
                    0 as c_int,
                    cb.p,
                ),
                "Screen()",
            )?;
            let profile = checked(
                api,
                (api.get_attr)(screen.p, c"color_profile".as_ptr()),
                "Screen.color_profile",
            )?;
            if (api.set_attr)(cb.p, c"color_profile".as_ptr(), profile.p) != 0 {
                (api.err_print)();
                return Err(Error::Python {
                    call: "Callbacks.color_profile =".to_string(),
                });
            }
            let method = |name: &CStr, call: &str| {
                checked(api, (api.get_attr)(screen.p, name.as_ptr()), call)
            };
            let create = method(
                c"test_create_write_buffer",
                "Screen.test_create_write_buffer",
            )?;
            let commit = method(
                c"test_commit_write_buffer",
                "Screen.test_commit_write_buffer",
            )?;
            let parse = method(c"test_parse_written_data", "Screen.test_parse_written_data")?;
            Ok(Screen {
                kitty: self,
                obj: screen,
                _callbacks: cb,
                create,
                commit,
                parse,
            })
        }
    }
}

/// Where the libpython holding `Py_InitializeEx` was loaded from.
fn python_library(api: &Api) -> Option<PathBuf> {
    // SAFETY: `info` is plain data dladdr fills; its name is NUL-terminated and
    // copied before any further loader call.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(api.initialize_ex as *const c_void, &mut info) == 0
            || info.dli_fname.is_null()
        {
            return None;
        }
        Some(PathBuf::from(OsStr::from_encoded_bytes_unchecked(
            CStr::from_ptr(info.dli_fname).to_bytes(),
        )))
    }
}

/// Turn a returned object into a `Ref`, or a null into the error it means.
unsafe fn checked(api: &'static Api, p: Obj, call: &str) -> Result<Ref, Error> {
    if p.is_null() {
        (api.err_print)();
        return Err(Error::Python {
            call: call.to_string(),
        });
    }
    Ok(Ref { api, p })
}

/// `type("Callbacks", (), {"__getattr__": any_attribute})`, by C API.
unsafe fn callbacks_class(api: &'static Api) -> Result<Ref, Error> {
    let new_function = |name: &'static CStr, f: *const c_void, flags: c_int, what: &str| {
        let def = Box::leak(Box::new(MethodDef {
            name: name.as_ptr(),
            method: f,
            flags,
            doc: std::ptr::null(),
        }));
        checked(api, (api.cfunction_new)(def, null_mut(), null_mut()), what)
    };
    let noop_fn = new_function(
        c"noop",
        noop as *const c_void,
        METH_VARARGS_KEYWORDS,
        "noop",
    )?;
    NOOP.store(noop_fn.p, Ordering::Relaxed);
    // Held for the life of the process, as the static above names it.
    std::mem::forget(noop_fn);
    let getattr = new_function(
        c"__getattr__",
        any_attribute as *const c_void,
        METH_O,
        "__getattr__",
    )?;
    let builtins = checked(
        api,
        (api.import_module)(c"builtins".as_ptr()),
        "import builtins",
    )?;
    let type_ = checked(
        api,
        (api.get_attr)(builtins.p, c"type".as_ptr()),
        "builtins.type",
    )?;
    checked(
        api,
        (api.call_function)(
            type_.p,
            c"s(){sO}".as_ptr(),
            c"Callbacks".as_ptr(),
            c"__getattr__".as_ptr(),
            getattr.p,
        ),
        "type(Callbacks)",
    )
}

/// A kitty `Screen`, with the three calls looked up once.
pub struct Screen<'k> {
    kitty: &'k Kitty,
    obj: Ref,
    _callbacks: Ref,
    create: Ref,
    commit: Ref,
    parse: Ref,
}

impl Cycle for Screen<'_> {
    type Dest = Ref;

    fn create(&mut self) -> Result<Ref, Error> {
        let api = self.kitty.api;
        // SAFETY: the interpreter is up on this thread; the list ends in null.
        unsafe {
            checked(
                api,
                (api.call_obj_args)(self.create.p, null_mut::<c_void>()),
                "test_create_write_buffer",
            )
        }
    }

    fn commit(&mut self, data: &[u8], dest: &Ref) -> Result<usize, Error> {
        let api = self.kitty.api;
        // SAFETY: the memoryview is read-only over `data`, and it is released
        // before this returns, so nothing keeps `data` past its borrow. The
        // argument list ends in null.
        unsafe {
            let view = checked(
                api,
                (api.memoryview_from_memory)(
                    data.as_ptr().cast_mut().cast(),
                    data.len() as isize,
                    PYBUF_READ,
                ),
                "memoryview",
            )?;
            let n = checked(
                api,
                (api.call_obj_args)(self.commit.p, view.p, dest.p, null_mut::<c_void>()),
                "test_commit_write_buffer",
            )?;
            let v = (api.long_as_ssize_t)(n.p);
            if v < 0 {
                if !(api.err_occurred)().is_null() {
                    (api.err_print)();
                    return Err(Error::Python {
                        call: "test_commit_write_buffer's count".to_string(),
                    });
                }
                return Err(Error::BrokenCycle {
                    offered: data.len(),
                    committed: 0,
                });
            }
            Ok(v as usize)
        }
    }

    fn parse(&mut self) -> Result<(), Error> {
        let api = self.kitty.api;
        // SAFETY: the interpreter is up on this thread; the list ends in null.
        unsafe {
            checked(
                api,
                (api.call_obj_args)(self.parse.p, null_mut::<c_void>()),
                "test_parse_written_data",
            )
            .map(drop)
        }
    }
}

impl Screen<'_> {
    /// All of `chunk`, through the three calls.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<usize, Error> {
        cycle::feed(self, chunk)
    }

    /// The text of line `y`, from `Screen.line`.
    pub fn line(&self, y: c_int) -> Result<String, Error> {
        let api = self.kitty.api;
        // SAFETY: the interpreter is up on this thread; the format matches, and
        // the UTF-8 pointer is valid while `text` is held.
        unsafe {
            let line = checked(
                api,
                (api.call_method)(self.obj.p, c"line".as_ptr(), c"i".as_ptr(), y),
                "Screen.line",
            )?;
            let text = checked(api, (api.object_str)(line.p), "str(line)")?;
            let utf8 = (api.unicode_as_utf8)(text.p);
            if utf8.is_null() {
                (api.err_print)();
                return Err(Error::Python {
                    call: "PyUnicode_AsUTF8".to_string(),
                });
            }
            Ok(CStr::from_ptr(utf8).to_string_lossy().into_owned())
        }
    }

    /// The cursor's column and line, from `Screen.cursor`.
    pub fn cursor(&self) -> Result<(isize, isize), Error> {
        let api = self.kitty.api;
        // SAFETY: the interpreter is up on this thread and each reference is
        // checked before use.
        unsafe {
            let c = checked(
                api,
                (api.get_attr)(self.obj.p, c"cursor".as_ptr()),
                "Screen.cursor",
            )?;
            let x = checked(api, (api.get_attr)(c.p, c"x".as_ptr()), "cursor.x")?;
            let y = checked(api, (api.get_attr)(c.p, c"y".as_ptr()), "cursor.y")?;
            Ok(((api.long_as_ssize_t)(x.p), (api.long_as_ssize_t)(y.p)))
        }
    }

    /// Commit into something that is not a write buffer: the positive control
    /// that a broken cycle comes back as an error.
    pub fn commit_into(&mut self, data: &[u8], dest: &Ref) -> Result<usize, Error> {
        Cycle::commit(self, data, dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn the_home_is_above_lib_when_the_stdlib_is_there() {
        let t = TestDir::new("home-lib");
        t.write("lib/python3.14/os.py", b"");
        let lib = t.write("lib/libpython3.14.dylib", b"");
        assert_eq!(
            python_home(&lib, "3.14.7 (main, Aug  5 2026)").as_deref(),
            Some(t.path())
        );
    }

    #[test]
    fn a_framework_library_has_its_home_beside_it() {
        let t = TestDir::new("home-framework");
        t.write("Versions/3.13/lib/python3.13/os.py", b"");
        let lib = t.write("Versions/3.13/Python", b"");
        assert_eq!(
            python_home(&lib, "3.13.1").as_deref(),
            Some(t.path().join("Versions/3.13").as_path())
        );
    }

    #[test]
    fn no_stdlib_of_that_version_means_no_home() {
        let t = TestDir::new("home-none");
        t.write("lib/python3.13/os.py", b"");
        let lib = t.write("lib/libpython3.14.dylib", b"");
        assert_eq!(python_home(&lib, "3.14.7"), None);
    }

    #[test]
    fn a_version_with_no_minor_means_no_home() {
        assert_eq!(python_home(Path::new("/lib/libpython.so"), "3"), None);
    }

    #[test]
    fn the_callers_python_settings_are_removed_and_the_package_root_is_set() {
        let (remove, set) = interpreter_env(
            ["PYTHONPATH", "PYTHONHOME", "PYTHONSTARTUP", "PATH", "HOME"].map(OsString::from),
            Path::new("/k"),
            Some(Path::new("/py")),
        );
        assert_eq!(
            remove,
            ["PYTHONPATH", "PYTHONHOME", "PYTHONSTARTUP"].map(OsString::from)
        );
        let get = |k: &str| set.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(get("PYTHONPATH"), Some(OsString::from("/k")));
        assert_eq!(get("PYTHONHOME"), Some(OsString::from("/py")));
        assert_eq!(get("PYTHONDONTWRITEBYTECODE"), Some(OsString::from("1")));
        assert_eq!(get("PYTHONNOUSERSITE"), Some(OsString::from("1")));
    }

    #[test]
    fn no_home_found_sets_none() {
        let (_, set) = interpreter_env(Vec::new(), Path::new("/k"), None);
        assert!(set.iter().all(|(n, _)| *n != "PYTHONHOME"), "{set:?}");
    }

    #[test]
    fn a_library_that_will_not_load_is_refused_by_name() {
        let t = TestDir::new("not-a-library");
        let so = t.write("kitty/fast_data_types.so", b"not a library");
        let e = open_library(&so).unwrap_err();
        assert!(
            matches!(&e, Error::Load { library, .. } if *library == so),
            "{e:?}"
        );
    }

    #[test]
    fn a_named_libpython_that_will_not_load_is_refused_before_the_extension() {
        let t = TestDir::new("named-libpython");
        let named = t.write("libpython.so", b"not a library");
        let location = Location {
            package_root: t.path().to_path_buf(),
            extension: t.path().join("absent.so"),
        };
        let e = Kitty::load(&location, Some(&named)).err().expect("refused");
        assert!(
            matches!(&e, Error::Load { library, .. } if *library == named),
            "{e:?}"
        );
    }

    #[cfg(target_os = "macos")]
    const NOT_PYTHON: &str = "/usr/lib/libSystem.B.dylib";
    #[cfg(not(target_os = "macos"))]
    const NOT_PYTHON: &str = "libc.so.6";

    #[test]
    fn a_library_that_is_not_python_is_refused_by_its_first_missing_symbol() {
        let h = open_library(Path::new(NOT_PYTHON)).unwrap();
        let e = Api::load(&h).err().expect("libc has no Py_InitializeEx");
        assert!(
            matches!(&e, Error::NotPython { symbol, .. } if symbol == "Py_InitializeEx"),
            "{e:?}"
        );
        assert!(e.to_string().contains("KITTY_LIBPYTHON"), "{e}");
    }
}
