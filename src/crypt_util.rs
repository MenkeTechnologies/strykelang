//! Unix `crypt(3)` wrapper for Perl `crypt` (DES / system hashing).

/// Hash `plaintext` with `salt` using the platform libc `crypt`.
/// On non-Unix targets, returns an empty string.
pub fn perl_crypt(plaintext: &str, salt: &str) -> String {
    #[cfg(unix)]
    {
        use std::ffi::{CStr, CString};

        extern "C" {
            fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
        }

        unsafe {
            let key = match CString::new(plaintext.as_bytes()) {
                Ok(s) => s,
                Err(_) => return String::new(),
            };
            let salt = match CString::new(salt.as_bytes()) {
                Ok(s) => s,
                Err(_) => return String::new(),
            };
            let ptr = crypt(key.as_ptr(), salt.as_ptr());
            if ptr.is_null() {
                return String::new();
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (plaintext, salt);
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CString::new` rejects interior NUL; `crypt` must not be called with invalid C strings.
    #[test]
    fn crypt_plaintext_with_nul_returns_empty() {
        assert!(perl_crypt("a\0b", "xx").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn crypt_valid_inputs_yield_non_empty_hash() {
        let h = perl_crypt("secret", "ab");
        assert!(
            !h.is_empty(),
            "platform crypt(3) should hash with a two-character salt"
        );
    }
}
