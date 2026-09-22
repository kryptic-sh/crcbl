//! The fake library the unit tests run against. Test builds only.
//!
//! A [`Lib`] whose function pointers are Rust `extern "C"` functions reading
//! and writing a thread-local [`Script`]: what init answers, whether each
//! accessor returns an interface or null, what the callback pipe yields, and
//! counters for every call that matters. The pointers have the exact types
//! `ffi::manifest` declares, so the code under test calls through the same
//! signatures it calls the real library through.
//!
//! **What this cannot prove** is that those signatures match C: a Rust callee
//! compiled from the same declaration agrees with its caller by construction.
//! That is the drift gate's job for types, and a real client's for the calling
//! convention.
//!
//! A function pointer captures nothing, hence the thread-local; each unit test
//! runs on its own thread (or, under nextest, its own process), so each starts
//! from [`Script::default`]. Each test also leaks its own [`Lib`], so the
//! one-live-`Steam` guard is per test.

use std::{cell::RefCell, collections::VecDeque, ffi::c_void, ptr::NonNull, sync::Mutex};

use crate::ffi::{
    HSteamPipe, ISteamUser, ISteamUtils, Lib, SteamErrMsg,
    manifest::{DispatchFns, Fns, LifecycleFns, UserFns, UtilsFns},
    structs::CallbackMsg,
};

/// The pipe the fake hands out.
pub(crate) const PIPE: HSteamPipe = 7;
/// The local user the fake reports.
pub(crate) const STEAM_ID: u64 = 76_561_197_960_287_930;

/// One message the fake pipe will yield.
#[derive(Debug, Clone)]
pub(crate) struct FakeMsg {
    id: i32,
    /// `None` hands over a null `m_pubParam`.
    payload: Option<Vec<u8>>,
    size: i32,
}

impl FakeMsg {
    /// A message whose size is its payload's length.
    pub(crate) fn payload(id: i32, bytes: Vec<u8>) -> Self {
        let size = i32::try_from(bytes.len()).unwrap();
        Self {
            id,
            payload: Some(bytes),
            size,
        }
    }

    /// A message with a null payload pointer and the given size.
    pub(crate) fn null(id: i32, size: i32) -> Self {
        Self {
            id,
            payload: None,
            size,
        }
    }

    /// Appends a byte to the payload, growing the reported size with it.
    pub(crate) fn push_byte(&mut self, byte: u8) {
        if let Some(bytes) = &mut self.payload {
            bytes.push(byte);
            self.size += 1;
        }
    }
}

/// How often each fake function ran, and each protocol violation seen.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Calls {
    pub(crate) init: u32,
    pub(crate) shutdown: u32,
    pub(crate) dispatch_init: u32,
    pub(crate) run_frame: u32,
    pub(crate) release_thread_memory: u32,
    /// `GetNextCallback` answering true.
    pub(crate) next_true: u32,
    pub(crate) free: u32,
    /// `FreeLastCallback` with no message outstanding.
    pub(crate) free_without_next: u32,
    /// `GetNextCallback` while the previous message was never freed.
    pub(crate) next_while_unfreed: u32,
    /// A dispatch call on a pipe the fake never handed out.
    pub(crate) wrong_pipe: u32,
}

/// What the fake answers, and what it has seen.
#[derive(Debug)]
pub(crate) struct Script {
    pub(crate) init_result: i32,
    pub(crate) init_message: Vec<u8>,
    /// The handshake `SteamInternal_SteamAPI_Init` received, through its
    /// double NUL.
    pub(crate) handshake: Option<Vec<u8>>,
    pub(crate) null_user: bool,
    pub(crate) null_utils: bool,
    pub(crate) logged_on: bool,
    pub(crate) app_id: u32,
    pub(crate) hardware: i32,
    /// What the pipe yields, in order.
    pub(crate) queue: VecDeque<FakeMsg>,
    /// The outstanding message's payload, alive until `FreeLastCallback` —
    /// so a pump that read it after the free would read freed memory, which
    /// Miri reports.
    current: Option<Option<Vec<u8>>>,
    pub(crate) calls: Calls,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            init_result: crate::ffi::init_result::OK,
            init_message: Vec::new(),
            handshake: None,
            null_user: false,
            null_utils: false,
            logged_on: true,
            app_id: 480,
            hardware: 0,
            queue: VecDeque::new(),
            current: None,
            calls: Calls::default(),
        }
    }
}

thread_local! {
    static SCRIPT: RefCell<Script> = RefCell::new(Script::default());
}

/// Reads or edits this thread's script.
pub(crate) fn script<R>(f: impl FnOnce(&mut Script) -> R) -> R {
    SCRIPT.with(|cell| f(&mut cell.borrow_mut()))
}

/// Every fake library this process has made. The real library is leaked and
/// kept reachable from `load::real`'s static; this does the same for the
/// fakes, so Miri's leak check stays on for everything else the tests do.
static FAKES: Mutex<Vec<&'static Lib>> = Mutex::new(Vec::new());

/// A fresh fake library, leaked like the real one.
pub(crate) fn fake_lib() -> &'static Lib {
    let lib: &'static Lib = Box::leak(Box::new(Lib::new(Fns {
        lifecycle: LifecycleFns {
            init: fake_init,
            shutdown: fake_shutdown,
            get_pipe: fake_get_pipe,
            release_thread_memory: fake_release_thread_memory,
        },
        dispatch: DispatchFns {
            init: fake_dispatch_init,
            run_frame: fake_run_frame,
            get_next_callback: fake_get_next_callback,
            free_last_callback: fake_free_last_callback,
        },
        user: UserFns {
            accessor: fake_user_accessor,
            get_steam_id: fake_get_steam_id,
            logged_on: fake_logged_on,
        },
        utils: UtilsFns {
            accessor: fake_utils_accessor,
            get_app_id: fake_get_app_id,
            is_running_on_steam_hardware: fake_is_running_on_steam_hardware,
        },
    })));
    FAKES.lock().unwrap().push(lib);
    lib
}

/// A non-null interface pointer the fake never dereferences.
fn sentinel() -> *mut c_void {
    NonNull::<u64>::dangling().as_ptr().cast()
}

unsafe extern "C" fn fake_init(versions: *const core::ffi::c_char, message: *mut SteamErrMsg) -> i32 {
    // SAFETY: the caller passes a double-NUL-terminated list; read up to and
    // including the second NUL of a pair.
    let handshake = unsafe {
        let mut bytes = Vec::new();
        let mut at = versions.cast::<u8>();
        loop {
            let byte = *at;
            bytes.push(byte);
            if byte == 0 && (bytes.len() == 1 || bytes[bytes.len() - 2] == 0) {
                break;
            }
            at = at.add(1);
        }
        bytes
    };
    script(|s| {
        s.calls.init += 1;
        s.handshake = Some(handshake);
        // SAFETY: the caller passes a writable `SteamErrMsg`.
        let out = unsafe { &mut *message };
        for (slot, &byte) in out.iter_mut().zip(&s.init_message) {
            *slot = core::ffi::c_char::from_ne_bytes([byte]);
        }
        s.init_result
    })
}

unsafe extern "C" fn fake_shutdown() {
    script(|s| s.calls.shutdown += 1);
}

unsafe extern "C" fn fake_get_pipe() -> HSteamPipe {
    PIPE
}

unsafe extern "C" fn fake_release_thread_memory() {
    script(|s| s.calls.release_thread_memory += 1);
}

unsafe extern "C" fn fake_dispatch_init() {
    script(|s| s.calls.dispatch_init += 1);
}

unsafe extern "C" fn fake_run_frame(pipe: HSteamPipe) {
    script(|s| {
        s.calls.run_frame += 1;
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
    });
}

unsafe extern "C" fn fake_get_next_callback(pipe: HSteamPipe, msg: *mut CallbackMsg) -> bool {
    script(|s| {
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
        if s.current.is_some() {
            s.calls.next_while_unfreed += 1;
        }
        let Some(next) = s.queue.pop_front() else {
            return false;
        };
        s.calls.next_true += 1;
        let current = s.current.insert(next.payload);
        let param = current
            .as_mut()
            .map_or(core::ptr::null_mut(), |bytes| bytes.as_mut_ptr());
        // SAFETY: the caller passes a writable `CallbackMsg_t`.
        unsafe {
            msg.write(CallbackMsg {
                steam_user: 1,
                callback: next.id,
                param,
                param_size: next.size,
            });
        }
        true
    })
}

unsafe extern "C" fn fake_free_last_callback(pipe: HSteamPipe) {
    script(|s| {
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
        s.calls.free += 1;
        if s.current.take().is_none() {
            s.calls.free_without_next += 1;
        }
    });
}

unsafe extern "C" fn fake_user_accessor() -> *mut c_void {
    if script(|s| s.null_user) {
        core::ptr::null_mut()
    } else {
        sentinel()
    }
}

unsafe extern "C" fn fake_get_steam_id(_: *mut ISteamUser) -> u64 {
    STEAM_ID
}

unsafe extern "C" fn fake_logged_on(_: *mut ISteamUser) -> bool {
    script(|s| s.logged_on)
}

unsafe extern "C" fn fake_utils_accessor() -> *mut c_void {
    if script(|s| s.null_utils) {
        core::ptr::null_mut()
    } else {
        sentinel()
    }
}

unsafe extern "C" fn fake_get_app_id(_: *mut ISteamUtils) -> u32 {
    script(|s| s.app_id)
}

unsafe extern "C" fn fake_is_running_on_steam_hardware(_: *mut ISteamUtils) -> i32 {
    script(|s| s.hardware)
}
