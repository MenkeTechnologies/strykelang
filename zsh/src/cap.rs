//! Capabilities module - port of Modules/cap.c
//!
//! Provides POSIX.1e capability manipulation via cap, getcap, setcap builtins.

use std::io;

/// Get process capabilities
#[cfg(target_os = "linux")]
pub fn get_proc_caps() -> io::Result<String> {
    use std::ffi::CStr;

    unsafe {
        let caps = libc::cap_get_proc();
        if caps.is_null() {
            return Err(io::Error::last_os_error());
        }

        let text = libc::cap_to_text(caps, std::ptr::null_mut());
        if text.is_null() {
            libc::cap_free(caps as *mut libc::c_void);
            return Err(io::Error::last_os_error());
        }

        let result = CStr::from_ptr(text).to_string_lossy().into_owned();
        libc::cap_free(text as *mut libc::c_void);
        libc::cap_free(caps as *mut libc::c_void);

        Ok(result)
    }
}

#[cfg(not(target_os = "linux"))]
pub fn get_proc_caps() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "capabilities not supported",
    ))
}

/// Set process capabilities
#[cfg(target_os = "linux")]
pub fn set_proc_caps(cap_string: &str) -> io::Result<()> {
    use std::ffi::CString;

    let cap_c = CString::new(cap_string)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid capability string"))?;

    unsafe {
        let caps = libc::cap_from_text(cap_c.as_ptr());
        if caps.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid capability string",
            ));
        }

        let result = libc::cap_set_proc(caps);
        libc::cap_free(caps as *mut libc::c_void);

        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_proc_caps(_cap_string: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "capabilities not supported",
    ))
}

/// Get file capabilities
#[cfg(target_os = "linux")]
pub fn get_file_caps(path: &str) -> io::Result<String> {
    use std::ffi::{CStr, CString};

    let path_c = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;

    unsafe {
        let caps = libc::cap_get_file(path_c.as_ptr());
        if caps.is_null() {
            return Err(io::Error::last_os_error());
        }

        let text = libc::cap_to_text(caps, std::ptr::null_mut());
        if text.is_null() {
            libc::cap_free(caps as *mut libc::c_void);
            return Err(io::Error::last_os_error());
        }

        let result = CStr::from_ptr(text).to_string_lossy().into_owned();
        libc::cap_free(text as *mut libc::c_void);
        libc::cap_free(caps as *mut libc::c_void);

        Ok(result)
    }
}

#[cfg(not(target_os = "linux"))]
pub fn get_file_caps(_path: &str) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "capabilities not supported",
    ))
}

/// Set file capabilities
#[cfg(target_os = "linux")]
pub fn set_file_caps(cap_string: &str, path: &str) -> io::Result<()> {
    use std::ffi::CString;

    let cap_c = CString::new(cap_string)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid capability string"))?;
    let path_c = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;

    unsafe {
        let caps = libc::cap_from_text(cap_c.as_ptr());
        if caps.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid capability string",
            ));
        }

        let result = libc::cap_set_file(path_c.as_ptr(), caps);
        libc::cap_free(caps as *mut libc::c_void);

        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_file_caps(_cap_string: &str, _path: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "capabilities not supported",
    ))
}

/// Execute cap builtin
pub fn builtin_cap(args: &[&str]) -> (i32, String) {
    if args.is_empty() {
        match get_proc_caps() {
            Ok(caps) => (0, format!("{}\n", caps)),
            Err(e) => (1, format!("cap: {}\n", e)),
        }
    } else {
        match set_proc_caps(args[0]) {
            Ok(()) => (0, String::new()),
            Err(e) => (1, format!("cap: {}\n", e)),
        }
    }
}

/// Execute getcap builtin
pub fn builtin_getcap(args: &[&str]) -> (i32, String) {
    if args.is_empty() {
        return (1, "getcap: file required\n".to_string());
    }

    let mut output = String::new();
    let mut status = 0;

    for file in args {
        match get_file_caps(file) {
            Ok(caps) => output.push_str(&format!("{} {}\n", file, caps)),
            Err(e) => {
                output.push_str(&format!("getcap: {}: {}\n", file, e));
                status = 1;
            }
        }
    }

    (status, output)
}

/// Execute setcap builtin
pub fn builtin_setcap(args: &[&str]) -> (i32, String) {
    if args.len() < 2 {
        return (
            1,
            "setcap: capability string and file required\n".to_string(),
        );
    }

    let cap_string = args[0];
    let mut status = 0;
    let mut output = String::new();

    for file in &args[1..] {
        if let Err(e) = set_file_caps(cap_string, file) {
            output.push_str(&format!("setcap: {}: {}\n", file, e));
            status = 1;
        }
    }

    (status, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_cap_no_args() {
        let (status, _) = builtin_cap(&[]);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(status, 1);
    }

    #[test]
    fn test_builtin_getcap_no_args() {
        let (status, _) = builtin_getcap(&[]);
        assert_eq!(status, 1);
    }

    #[test]
    fn test_builtin_setcap_no_args() {
        let (status, _) = builtin_setcap(&[]);
        assert_eq!(status, 1);
    }

    #[test]
    fn test_builtin_setcap_missing_file() {
        let (status, _) = builtin_setcap(&["cap_net_admin+ep"]);
        assert_eq!(status, 1);
    }
}
