//! Unix `crypt(3)` wrapper for Perl `crypt` (DES / system hashing).

// On Linux glibc, `crypt(3)` is in libcrypt; the default link line does
// not include it, so lld fails with undefined `crypt` unless we name the
// library explicitly. Skipped on musl: the only libcrypt.a present in
// CI is glibc-built and references fortified glibc-only symbols
// (__snprintf_chk, __explicit_bzero_chk, __isoc23_strtoul) that don't
// exist in musl → undefined-reference storm. `perl_crypt` returns "" on
// musl, which is acceptable for a portable static binary.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
#[link(name = "crypt")]
unsafe extern "C" {}

/// Hash `plaintext` with `salt` using the platform libc `crypt`.
/// On non-Unix targets and on musl Linux, returns an empty string.
pub fn perl_crypt(plaintext: &str, salt: &str) -> String {
    #[cfg(all(unix, not(target_env = "musl")))]
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
    #[cfg(any(not(unix), target_env = "musl"))]
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
