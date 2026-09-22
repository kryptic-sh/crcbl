//! Strings Steam hands back.
//!
//! A `const char *` return is Steam's buffer, valid until the next call into
//! the API — often the same buffer, reused. So every one is copied into an
//! owned `String` before the function that received it returns, and nothing in
//! this crate ever hands out a borrow of Steam's memory.
//!
//! Steam's strings are meant to be UTF-8. One that is not is read lossily
//! (`U+FFFD` for each bad sequence) rather than refused, and counted in
//! [`PumpDiagnostics::lossy_strings`](crate::PumpDiagnostics::lossy_strings),
//! so a smoke test can say whether it ever happened.

use core::ffi::{CStr, c_char};

use crate::Steam;

impl Steam {
    /// Copies a string Steam just returned.
    ///
    /// A null pointer — which no binding in this crate is documented to
    /// return — reads as empty and is counted with the lossy strings, since
    /// it is equally a string Steam did not hand over intact.
    ///
    /// # Safety
    ///
    /// `ptr` is null or points at a NUL-terminated string that stays valid
    /// until this returns: call it on a pointer straight out of a Steam call,
    /// before any other Steam call.
    pub(crate) unsafe fn copy_string(&self, ptr: *const c_char) -> String {
        if ptr.is_null() {
            self.lossy_strings.set(self.lossy_strings.get() + 1);
            return String::new();
        }
        // SAFETY: the caller promises a live NUL-terminated string; `CStr`
        // reads up to the NUL and the bytes are copied before returning.
        let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
        match core::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(_) => {
                self.lossy_strings.set(self.lossy_strings.get() + 1);
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn a_returned_string_is_copied_before_steam_can_reuse_its_buffer() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.set_string(b"Gordon"));
        let name = steam.friends().persona_name();
        // The fake answers every string call out of one buffer; overwriting it
        // in place, as Steam's next call may, is what a borrow would now read.
        testing::script(|s| s.set_string(b"Alyx!!"));
        assert_eq!(name, "Gordon");
        assert_eq!(steam.diagnostics().lossy_strings, 0);
    }

    #[test]
    fn invalid_utf8_is_read_lossily_and_counted() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.set_string(b"caf\xE9"));
        assert_eq!(steam.apps().game_language(), "caf\u{FFFD}");
        assert_eq!(steam.diagnostics().lossy_strings, 1);
        testing::script(|s| s.set_string(b"english"));
        assert_eq!(steam.apps().game_language(), "english");
        assert_eq!(
            steam.diagnostics().lossy_strings,
            1,
            "valid UTF-8 is not lossy"
        );
    }

    #[test]
    fn a_null_string_is_empty_and_counted() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.null_string = true);
        assert_eq!(steam.utils().ip_country(), "");
        assert_eq!(steam.diagnostics().lossy_strings, 1);
    }
}
