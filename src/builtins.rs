//! Builtins dispatched from `FuncCall` (names not modeled as dedicated `ExprKind`s).
//! I/O uses `Interpreter::io_file_slots` for raw `read`/`write`/`seek` alongside buffered handles.

/// TCP/UDP socket storage (high-level `std::net`, not raw POSIX).
pub(crate) enum PerlSocket {
    Listener(TcpListener),
    Stream(TcpStream),
    #[allow(dead_code)]
    Udp(UdpSocket),
}

use std::io::{stderr, IsTerminal, Read, Seek, SeekFrom, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;

#[cfg(unix)]
use std::ffi::{CStr, CString};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

use crate::error::{PerlError, PerlResult};
use crate::interpreter::{Interpreter, LogLevelFilter, WantarrayCtx};
use crate::perl_decode::decode_utf8_or_latin1;
use crate::perl_regex::perl_quotemeta;
use crate::value::{PerlAsyncTask, PerlValue};

#[inline]
fn is_http_opts_hash(v: &PerlValue) -> bool {
    !v.is_undef() && (v.as_hash_map().is_some() || v.as_hash_ref().is_some())
}

#[inline]
fn opt_hash_bool(v: &PerlValue, key: &str) -> bool {
    let entry = if let Some(m) = v.as_hash_map() {
        m.get(key).cloned()
    } else if let Some(r) = v.as_hash_ref() {
        r.read().get(key).cloned()
    } else {
        return false;
    };
    entry.is_some_and(|x| x.is_true())
}

fn perl_scalar_as_bytes(v: &PerlValue) -> Vec<u8> {
    if let Some(b) = v.as_bytes_arc() {
        return b.as_ref().clone();
    }
    v.to_string().into_bytes()
}

fn builtin_spurt(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("spurt needs PATH and CONTENT", line));
    }
    let path = args[0].to_string();
    let data = perl_scalar_as_bytes(&args[1]);
    let opts = args.get(2);
    let mkdir = opts.is_some_and(|o| opt_hash_bool(o, "mkdir") || opt_hash_bool(o, "mkpath"));
    let atomic = opts.is_some_and(|o| opt_hash_bool(o, "atomic"));
    crate::perl_fs::spurt_path(&path, &data, mkdir, atomic)
        .map_err(|e| PerlError::runtime(format!("spurt: {}", e), line))?;
    Ok(PerlValue::integer(data.len() as i64))
}

fn builtin_copy(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("copy needs FROM and TO paths", line));
    }
    let from = args[0].to_string();
    let to = args[1].to_string();
    let preserve = args
        .get(2)
        .is_some_and(|o| opt_hash_bool(o, "preserve") || opt_hash_bool(o, "metadata"));
    Ok(crate::perl_fs::copy_file(&from, &to, preserve))
}

fn builtin_basename(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(crate::perl_fs::path_basename(&s)))
}

fn builtin_dirname(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(crate::perl_fs::path_dirname(&s)))
}

fn builtin_fileparse(
    interp: &Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let path = args
        .first()
        .ok_or_else(|| PerlError::runtime("fileparse needs a path", line))?;
    let suf = args.get(1).filter(|v| !v.is_undef()).map(|v| v.to_string());
    let (base, dir, sfx) = crate::perl_fs::fileparse_path(&path.to_string(), suf.as_deref());
    match interp.wantarray_kind {
        WantarrayCtx::List => Ok(PerlValue::array(vec![
            PerlValue::string(base),
            PerlValue::string(dir),
            PerlValue::string(sfx),
        ])),
        _ => Ok(PerlValue::string(base)),
    }
}

#[cfg(unix)]
fn builtin_gethostname() -> PerlResult<PerlValue> {
    let mut buf = vec![0u8; 512];
    let r = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if r != 0 {
        return Err(PerlError::runtime("gethostname failed", 0));
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Ok(PerlValue::string(
        String::from_utf8_lossy(&buf[..len]).into_owned(),
    ))
}

#[cfg(not(unix))]
fn builtin_gethostname() -> PerlResult<PerlValue> {
    Ok(PerlValue::string("localhost".into()))
}

#[cfg(unix)]
fn builtin_uname() -> PerlResult<PerlValue> {
    fn uts_field(slice: &[libc::c_char]) -> String {
        let n = slice.iter().take_while(|&&c| c != 0).count();
        let bytes: Vec<u8> = slice[..n].iter().map(|&c| c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } != 0 {
        return Err(PerlError::runtime("uname failed", 0));
    }
    let mut m = indexmap::IndexMap::new();
    m.insert(
        "sysname".into(),
        PerlValue::string(uts_field(uts.sysname.as_slice())),
    );
    m.insert(
        "nodename".into(),
        PerlValue::string(uts_field(uts.nodename.as_slice())),
    );
    m.insert(
        "release".into(),
        PerlValue::string(uts_field(uts.release.as_slice())),
    );
    m.insert(
        "version".into(),
        PerlValue::string(uts_field(uts.version.as_slice())),
    );
    m.insert(
        "machine".into(),
        PerlValue::string(uts_field(uts.machine.as_slice())),
    );
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(m))))
}

#[cfg(not(unix))]
fn builtin_uname() -> PerlResult<PerlValue> {
    Err(PerlError::runtime(
        "uname is not available on this platform",
        0,
    ))
}

/// If `name` is a known builtin, evaluate and return `Some`. Otherwise `None` (try user sub).
pub(crate) fn try_builtin(
    interp: &mut Interpreter,
    name: &str,
    args: &[PerlValue],
    line: usize,
) -> Option<PerlResult<PerlValue>> {
    let undef = PerlValue::UNDEF;
    match name {
        "basename" => Some(builtin_basename(args)),
        "copy" => Some(builtin_copy(args, line)),
        "dirname" => Some(builtin_dirname(args)),
        "fileparse" => Some(builtin_fileparse(interp, args, line)),
        "gethostname" => Some(builtin_gethostname()),
        "spurt" | "write_file" => Some(builtin_spurt(args, line)),
        "collect" => Some(interp.builtin_collect_execute(args, line)),
        "take" | "head" => {
            if name == "take"
                && args.len() == 2
                && args.first().and_then(|v| v.as_pipeline()).is_some()
            {
                let p = args[0].as_pipeline().expect("pipeline");
                return Some(interp.pipeline_method(
                    p,
                    "take",
                    std::slice::from_ref(&args[1]),
                    line,
                ));
            }
            Some(builtin_take(interp, args))
        }
        "tail" => Some(builtin_tail(interp, args)),
        "drop" => Some(builtin_drop(interp, args)),
        "take_while" | "drop_while" | "tap" | "peek" => {
            Some(interp.list_higher_order_block_builtin(name, args, line))
        }
        "with_index" => Some(builtin_with_index(interp, args)),
        "flatten" => Some(builtin_flatten(interp, args)),
        "set" => Some(Ok(crate::value::set_from_elements(args.iter().cloned()))),
        "list_count" | "list_size" => Some(builtin_list_count(args)),
        "uname" => Some(builtin_uname()),
        "rmdir" | "CORE::rmdir" => Some(interp.builtin_rmdir_execute(args, line)),
        "utime" | "CORE::utime" => Some(interp.builtin_utime_execute(args, line)),
        "umask" | "CORE::umask" => Some(interp.builtin_umask_execute(args, line)),
        "getcwd" | "CORE::getcwd" | "Cwd::getcwd" => {
            Some(interp.builtin_getcwd_execute(args, line))
        }
        "realpath" | "CORE::realpath" | "Cwd::realpath" => {
            Some(interp.builtin_realpath_execute(args, line))
        }
        "canonpath" => Some(builtin_canonpath(args)),
        "pipe" | "CORE::pipe" => Some(interp.builtin_pipe_execute(args, line)),
        "prototype" => Some(builtin_prototype(args)),
        "binmode" => Some(interp.builtin_binmode(args, line)),
        "fileno" => Some(interp.builtin_fileno(args, line)),
        "tell" => Some(interp.builtin_tell(args, line)),
        "CORE::tell" | "builtin::tell" => Some(interp.builtin_tell(args, line)),
        "flock" => Some(interp.builtin_flock(args, line)),
        "getc" => Some(interp.builtin_getc(args, line)),
        "readline" => Some({
            let handle = args.first().map(|v| v.to_string());
            interp.readline_builtin_execute(handle.as_deref())
        }),
        // Qualified names (`CORE::eof`, `builtin::eof`) parse as [`ExprKind::FuncCall`], not
        // [`ExprKind::Eof`]; must still see `-n`/`-p` line-mode EOF state.
        "CORE::eof" | "builtin::eof" => Some(interp.eof_builtin_execute(args, line)),
        "sysread" => Some(interp.builtin_sysread(args, line)),
        "syswrite" => Some(interp.builtin_syswrite(args, line)),
        "sysseek" => Some(interp.builtin_sysseek(args, line)),
        "truncate" => Some(interp.builtin_truncate(args, line)),
        "select" => Some(interp.builtin_select(args, line)),
        "fork" => Some(builtin_fork()),
        "wait" => Some(builtin_wait()),
        "waitpid" => Some(builtin_waitpid(args)),
        "ssh" => Some(interp.ssh_builtin_execute(args)),
        "kill" => Some(builtin_kill(args)),
        "alarm" => Some(builtin_alarm(args)),
        "sleep" => Some(builtin_sleep(args)),
        "times" => Some(builtin_times()),
        "time" | "CORE::time" => Some(builtin_time()),
        "localtime" | "CORE::localtime" => Some(interp.builtin_localtime(args, line)),
        "gmtime" | "CORE::gmtime" => Some(interp.builtin_gmtime(args, line)),
        "getlogin" | "CORE::getlogin" => Some(builtin_getlogin()),
        "getpwuid" | "CORE::getpwuid" => Some(interp.builtin_getpwuid(args, line)),
        "getpwnam" | "CORE::getpwnam" => Some(interp.builtin_getpwnam(args, line)),
        "getgrgid" | "CORE::getgrgid" => Some(interp.builtin_getgrgid(args, line)),
        "getgrnam" | "CORE::getgrnam" => Some(interp.builtin_getgrnam(args, line)),
        "getppid" | "CORE::getppid" => Some(builtin_getppid()),
        "getpgrp" | "CORE::getpgrp" => Some(builtin_getpgrp(args)),
        "setpgrp" | "CORE::setpgrp" => Some(builtin_setpgrp(args, line)),
        "getpriority" | "CORE::getpriority" => Some(builtin_getpriority(args, line)),
        "setpriority" | "CORE::setpriority" => Some(builtin_setpriority(args, line)),
        "gethostbyname" | "CORE::gethostbyname" => Some(interp.builtin_gethostbyname(args, line)),
        "getprotobyname" | "CORE::getprotobyname" => {
            Some(interp.builtin_getprotobyname(args, line))
        }
        "getservbyname" | "CORE::getservbyname" => Some(interp.builtin_getservbyname(args, line)),
        "setsockopt" | "CORE::setsockopt" => Some(interp.builtin_setsockopt(args, line)),
        "getsockopt" | "CORE::getsockopt" => Some(interp.builtin_getsockopt(args, line)),
        "getpeername" | "CORE::getpeername" => Some(interp.builtin_getpeername(args, line)),
        "getsockname" | "CORE::getsockname" => Some(interp.builtin_getsockname(args, line)),
        "socket" => Some(interp.builtin_socket(args, line)),
        "bind" => Some(interp.builtin_bind(args, line)),
        "listen" => Some(interp.builtin_listen(args, line)),
        "accept" => Some(interp.builtin_accept(args, line)),
        "connect" => Some(interp.builtin_connect(args, line)),
        "send" => Some(interp.builtin_send(args, line)),
        "recv" => Some(interp.builtin_recv(args, line)),
        "shutdown" => Some(interp.builtin_shutdown(args, line)),
        "pack" => Some(crate::pack::perl_pack(args, line)),
        "unpack" => Some(crate::pack::perl_unpack(args, line)),
        "quotemeta" => Some(builtin_quotemeta(args)),
        "pselect" => Some(crate::pchannel::pselect_recv(args, line)),
        "csv_read" => Some(builtin_csv_read(args)),
        "csv_write" => Some(builtin_csv_write(args)),
        "sqlite" => Some(builtin_sqlite(args)),
        "fetch" => Some(builtin_fetch(args, line)),
        "fetch_json" => Some(builtin_fetch_json(args, line)),
        "http_request" => Some(builtin_http_request(args, line)),
        "read_bytes" | "slurp_raw" => Some(builtin_read_bytes(args, line)),
        "move" | "mv" => Some(builtin_move(args, line)),
        "which" => Some(builtin_which(args, line)),
        "json_encode" => Some(builtin_json_encode(args)),
        "json_decode" => Some(builtin_json_decode(args)),
        "json_jq" => Some(builtin_json_jq(args)),
        "sha256" => Some(crate::native_codec::sha256(args.first().unwrap_or(&undef))),
        "md5" => Some(crate::native_codec::md5_digest(
            args.first().unwrap_or(&undef),
        )),
        "sha1" => Some(crate::native_codec::sha1_digest(
            args.first().unwrap_or(&undef),
        )),
        "hmac_sha256" | "hmac" => Some({
            let key = args.first().unwrap_or(&undef);
            let msg = args.get(1).unwrap_or(&undef);
            crate::native_codec::hmac_sha256(key, msg)
        }),
        "uuid" => Some(crate::native_codec::uuid_v4()),
        "base64_encode" => Some(crate::native_codec::base64_encode(
            args.first().unwrap_or(&undef),
        )),
        "base64_decode" => Some(crate::native_codec::base64_decode(
            args.first().unwrap_or(&undef),
        )),
        "hex_encode" => Some(crate::native_codec::hex_encode(
            args.first().unwrap_or(&undef),
        )),
        "hex_decode" => Some(crate::native_codec::hex_decode(
            args.first().unwrap_or(&undef),
        )),
        "gzip" => Some(crate::native_codec::gzip(args.first().unwrap_or(&undef))),
        "gunzip" => Some(crate::native_codec::gunzip(args.first().unwrap_or(&undef))),
        "zstd" => Some(crate::native_codec::zstd_compress(
            args.first().unwrap_or(&undef),
        )),
        "zstd_decode" => Some(crate::native_codec::zstd_decode(
            args.first().unwrap_or(&undef),
        )),
        "datetime_utc" => Some(crate::native_codec::datetime_utc()),
        "datetime_from_epoch" => Some(crate::native_codec::datetime_from_epoch(
            args.first().unwrap_or(&undef),
        )),
        "datetime_parse_rfc3339" => Some(crate::native_codec::datetime_parse_rfc3339(
            args.first().unwrap_or(&undef),
        )),
        "datetime_strftime" => Some({
            let a = args.first().unwrap_or(&undef);
            let b = args.get(1).unwrap_or(&undef);
            crate::native_codec::datetime_strftime(a, b)
        }),
        "datetime_now_tz" => Some(crate::native_codec::datetime_now_tz(
            args.first().unwrap_or(&undef),
        )),
        "datetime_format_tz" => Some(crate::native_codec::datetime_format_tz(
            args.first().unwrap_or(&undef),
            args.get(1).unwrap_or(&undef),
            args.get(2).unwrap_or(&undef),
        )),
        "datetime_parse_local" => Some(crate::native_codec::datetime_parse_local(
            args.first().unwrap_or(&undef),
            args.get(1).unwrap_or(&undef),
        )),
        "datetime_add_seconds" => Some(crate::native_codec::datetime_add_seconds(
            args.first().unwrap_or(&undef),
            args.get(1).unwrap_or(&undef),
        )),
        "toml_decode" => Some(builtin_toml_decode(args)),
        "toml_encode" => Some(builtin_toml_encode(args)),
        "xml_decode" => Some(builtin_xml_decode(args)),
        "xml_encode" => Some(builtin_xml_encode(args)),
        "yaml_decode" => Some(builtin_yaml_decode(args)),
        "yaml_encode" => Some(builtin_yaml_encode(args)),
        "url_encode" | "uri_escape" => Some(crate::native_codec::url_encode(
            args.first().unwrap_or(&undef),
        )),
        "url_decode" | "uri_unescape" => Some(crate::native_codec::url_decode(
            args.first().unwrap_or(&undef),
        )),
        // `async_fetch` would tokenize as keyword `async` — use `fetch_async` / `fetch_async_json`.
        "fetch_async" => Some(builtin_fetch_async(args)),
        "fetch_async_json" => Some(builtin_fetch_async_json(args)),
        "par_fetch" => Some(builtin_par_fetch(args)),
        "par_csv_read" => Some(builtin_par_csv_read(args)),
        "dataframe" => Some(builtin_dataframe(args)),
        "par_pipeline" => {
            if crate::par_pipeline::is_named_par_pipeline_args(args) {
                Some(crate::par_pipeline::run_par_pipeline(interp, args, line))
            } else {
                Some(interp.builtin_par_pipeline_stream(args, line))
            }
        }
        "par_pipeline_stream" => {
            if crate::par_pipeline::is_named_par_pipeline_args(args) {
                Some(crate::par_pipeline::run_par_pipeline_streaming(
                    interp, args, line,
                ))
            } else {
                Some(interp.builtin_par_pipeline_stream_new(args, line))
            }
        }
        "jwt_encode" => Some(builtin_jwt_encode(args, line)),
        "jwt_decode" => Some(builtin_jwt_decode(args, line)),
        "jwt_decode_unsafe" => Some(builtin_jwt_decode_unsafe(args, line)),
        "log_info" => Some(builtin_log_line(interp, args, line, LogLevelFilter::Info)),
        "log_warn" => Some(builtin_log_line(interp, args, line, LogLevelFilter::Warn)),
        "log_error" => Some(builtin_log_line(interp, args, line, LogLevelFilter::Error)),
        "log_debug" => Some(builtin_log_line(interp, args, line, LogLevelFilter::Debug)),
        "log_trace" => Some(builtin_log_line(interp, args, line, LogLevelFilter::Trace)),
        "log_json" => Some(builtin_log_json(interp, args, line)),
        "log_level" => Some(builtin_log_level(interp, args, line)),
        "write" => Some(interp.write_format_execute(args, line)),
        // Rust FFI entry points (see `src/rust_ffi.rs`).
        // `__perlrs_rust_compile(BASE64_BODY, LINE)` is generated by `rust_sugar` for each
        // `rust { ... }` block and invoked from a BEGIN block at parse time. It is not
        // intended for direct user use.
        "__perlrs_rust_compile" => {
            let body = args.first().map(|v| v.to_string()).unwrap_or_default();
            let bline = args.get(1).map(|v| v.to_int() as usize).unwrap_or(line);
            Some(crate::rust_ffi::compile_and_register(&body, bline).map(|()| PerlValue::UNDEF))
        }
        _ => crate::rust_ffi::try_call(name, args, line),
    }
}

fn jwt_hash_alg_opt(h: &PerlValue, line: usize) -> PerlResult<Option<String>> {
    if let Some(m) = h.as_hash_map() {
        let e = m.get("alg").filter(|x| !x.is_undef());
        return Ok(e.map(|x| x.to_string()));
    }
    if let Some(r) = h.as_hash_ref() {
        let g = r.read();
        let e = g.get("alg").filter(|x| !x.is_undef());
        return Ok(e.map(|x| x.to_string()));
    }
    Err(PerlError::runtime(
        "jwt_encode: options must be a hash or hashref",
        line,
    ))
}

fn builtin_jwt_encode(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let payload = args
        .first()
        .ok_or_else(|| PerlError::runtime("jwt_encode: need PAYLOAD, SECRET [, alg => …]", line))?;
    let secret = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("jwt_encode: need SECRET", line))?;
    let mut alg = "HS256".to_string();
    if let Some(t) = args.get(2) {
        if is_http_opts_hash(t) {
            if let Some(a) = jwt_hash_alg_opt(t, line)? {
                alg = a;
            }
            return crate::jwt::jwt_encode(payload, secret, &alg, line);
        }
    }
    let mut i = 2;
    while i + 1 < args.len() {
        if args[i].to_string() == "alg" {
            alg = args[i + 1].to_string();
        }
        i += 2;
    }
    crate::jwt::jwt_encode(payload, secret, &alg, line)
}

fn builtin_jwt_decode(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let token = args
        .first()
        .ok_or_else(|| PerlError::runtime("jwt_decode: need TOKEN, SECRET", line))?
        .to_string();
    let secret = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("jwt_decode: need SECRET", line))?;
    crate::jwt::jwt_decode(&token, secret, line)
}

fn builtin_jwt_decode_unsafe(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let token = args
        .first()
        .ok_or_else(|| PerlError::runtime("jwt_decode_unsafe: need TOKEN", line))?
        .to_string();
    crate::jwt::jwt_decode_unsafe(&token, line)
}

#[inline]
fn log_should_emit(level: LogLevelFilter, min: LogLevelFilter) -> bool {
    level >= min
}

fn log_level_style(level: LogLevelFilter, color: bool) -> &'static str {
    if !color {
        return "";
    }
    match level {
        LogLevelFilter::Trace => "\x1b[90m",
        LogLevelFilter::Debug => "\x1b[36m",
        LogLevelFilter::Info => "\x1b[32m",
        LogLevelFilter::Warn => "\x1b[33m",
        LogLevelFilter::Error => "\x1b[31m",
    }
}

fn format_log_hash_kv(v: &PerlValue, line: usize) -> PerlResult<String> {
    let mut parts = Vec::new();
    if let Some(m) = v.as_hash_map() {
        for (k, val) in m {
            let s = val.to_string().replace('\n', "\\n").replace('\r', "\\r");
            parts.push(format!("{k}={s}"));
        }
    } else if let Some(r) = v.as_hash_ref() {
        for (k, val) in r.read().iter() {
            let s = val.to_string().replace('\n', "\\n").replace('\r', "\\r");
            parts.push(format!("{k}={s}"));
        }
    } else {
        return Err(PerlError::runtime(
            "log_* optional second argument must be a hash or hashref",
            line,
        ));
    }
    Ok(parts.join(" "))
}

fn builtin_log_line(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
    level: LogLevelFilter,
) -> PerlResult<PerlValue> {
    if !log_should_emit(level, interp.log_filter_effective()) {
        return Ok(PerlValue::integer(0));
    }
    let msg = args
        .first()
        .ok_or_else(|| PerlError::runtime("log_* needs a message", line))?
        .to_string();
    let extra = if let Some(h) = args.get(1) {
        if h.is_undef() {
            String::new()
        } else {
            format!(" {}", format_log_hash_kv(h, line)?)
        }
    } else {
        String::new()
    };
    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let use_color = !interp.no_color_effective() && stderr().is_terminal();
    let tag = level.as_str().to_ascii_uppercase();
    let body = format!("{msg}{extra}");
    if use_color {
        let open = log_level_style(level, true);
        let _ = writeln!(stderr(), "{ts} {open}{tag}\x1b[0m {body}");
    } else {
        let _ = writeln!(stderr(), "{ts} [{tag}] {body}");
    }
    Ok(PerlValue::integer(1))
}

fn builtin_log_json(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let level_s = args
        .first()
        .ok_or_else(|| PerlError::runtime("log_json: need LEVEL, MSG [, \\%fields]", line))?
        .to_string();
    let level_f = LogLevelFilter::parse(&level_s)
        .ok_or_else(|| PerlError::runtime(format!("log_json: unknown level {level_s}"), line))?;
    if !log_should_emit(level_f, interp.log_filter_effective()) {
        return Ok(PerlValue::integer(0));
    }
    let msg = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("log_json: need MSG", line))?
        .to_string();
    let mut obj = serde_json::Map::new();
    if let Some(h) = args.get(2) {
        if !h.is_undef() {
            let j = crate::native_data::perl_to_json_value(h)?;
            match j {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        obj.insert(k, v);
                    }
                }
                _ => {
                    return Err(PerlError::runtime(
                        "log_json: third arg must be a hash or hashref",
                        line,
                    ));
                }
            }
        }
    }
    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    obj.insert("ts".into(), serde_json::Value::String(ts));
    obj.insert(
        "level".into(),
        serde_json::Value::String(level_f.as_str().to_string()),
    );
    obj.insert("msg".into(), serde_json::Value::String(msg));
    let line_s = serde_json::to_string(&serde_json::Value::Object(obj))
        .map_err(|e| PerlError::runtime(format!("log_json: {e}"), line))?;
    let _ = writeln!(stderr(), "{line_s}");
    Ok(PerlValue::integer(1))
}

fn builtin_log_level(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::string(
            interp.log_filter_effective().as_str().to_string(),
        ));
    }
    let a = &args[0];
    if a.is_undef() {
        interp.log_level_override = None;
        return Ok(PerlValue::string(
            interp.log_filter_effective().as_str().to_string(),
        ));
    }
    let name = a.to_string();
    let filt = LogLevelFilter::parse(&name)
        .ok_or_else(|| PerlError::runtime(format!("log_level: unknown level {name}"), line))?;
    interp.log_level_override = Some(filt);
    Ok(PerlValue::string(filt.as_str().to_string()))
}

fn builtin_dataframe(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    if path.is_empty() {
        return Err(PerlError::runtime("dataframe needs a file path", 0));
    }
    crate::native_data::dataframe_from_path(&path)
}

fn builtin_csv_read(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::csv_read(&path)
}

fn builtin_csv_write(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime("csv_write needs path and row list", 0));
    }
    let path = args[0].to_string();
    if args.len() == 2 {
        let v = &args[1];
        if crate::nanbox::is_heap(v.0) {
            match unsafe { v.heap_ref() } {
                crate::value::HeapObject::Array(a) => {
                    return crate::native_data::csv_write(&path, a);
                }
                crate::value::HeapObject::ArrayRef(r) => {
                    let g = r.read();
                    return crate::native_data::csv_write(
                        &path,
                        &g.iter().cloned().collect::<Vec<_>>(),
                    );
                }
                crate::value::HeapObject::Hash(h) => {
                    return crate::native_data::csv_write(&path, &[PerlValue::hash(h.clone())]);
                }
                crate::value::HeapObject::HashRef(r) => {
                    let g = r.read();
                    return crate::native_data::csv_write(&path, &[PerlValue::hash(g.clone())]);
                }
                _ => {}
            }
        }
    }
    crate::native_data::csv_write(&path, &args[1..])
}

fn builtin_sqlite(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::sqlite_open(&path)
}

fn builtin_fetch(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let url = args.first().map(|v| v.to_string()).unwrap_or_default();
    if let Some(opt) = args.get(1) {
        if !opt.is_undef() {
            if is_http_opts_hash(opt) {
                return crate::native_data::http_request(&url, Some(opt));
            }
            return Err(PerlError::runtime(
                "fetch: second argument must be a hash or hashref (method, headers, body, …)",
                line,
            ));
        }
    }
    crate::native_data::fetch(&url)
}

fn builtin_fetch_json(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let url = args.first().map(|v| v.to_string()).unwrap_or_default();
    if let Some(opt) = args.get(1) {
        if !opt.is_undef() {
            if is_http_opts_hash(opt) {
                let res = crate::native_data::http_request(&url, Some(opt))?;
                return crate::native_data::http_response_json_body(&res);
            }
            return Err(PerlError::runtime(
                "fetch_json: second argument must be a hash or hashref (method, headers, json, …)",
                line,
            ));
        }
    }
    crate::native_data::fetch_json(&url)
}

fn builtin_http_request(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let url = args
        .first()
        .filter(|v| !v.to_string().is_empty())
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("http_request needs a URL", line))?;
    crate::native_data::http_request(&url, args.get(1).filter(|v| !v.is_undef()))
}

fn builtin_read_bytes(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = crate::perl_fs::read_file_bytes(&path)
        .map_err(|e| PerlError::runtime(format!("read_bytes: {}", e), line))?;
    Ok(PerlValue::bytes(b))
}

fn builtin_move(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("move needs FROM and TO paths", line));
    }
    let from = args[0].to_string();
    let to = args[1].to_string();
    Ok(crate::perl_fs::move_path(&from, &to))
}

/// First `$n` elements: operands are **list values then count** (`take @l, N`); see
/// [`crate::list_util::head_tail_take_impl`]. Unary `take(N)` uses an empty list.
fn builtin_take(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    crate::list_util::head_tail_take_impl(
        args,
        crate::list_util::HeadTailTake::Take,
        interp.wantarray_kind,
    )
}

fn builtin_tail(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    crate::list_util::extension_tail_impl(args, interp.wantarray_kind)
}

fn builtin_drop(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    crate::list_util::extension_drop_impl(args, interp.wantarray_kind)
}

/// `list_count LIST` / `list_size LIST` — evaluate like [`builtin_flatten`] (list context per actual,
/// one-level [`PerlValue::map_flatten_outputs`]); always returns the element count as an integer.
fn builtin_list_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut out = Vec::new();
    for a in args {
        out.extend(a.map_flatten_outputs(true));
    }
    Ok(PerlValue::integer(out.len() as i64))
}

/// One-level list flatten: plain arrays and arrayrefs expand like `flat_map` / [`PerlValue::map_flatten_outputs`].
fn builtin_flatten(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut out = Vec::new();
    for a in args {
        out.extend(a.map_flatten_outputs(true));
    }
    Ok(match interp.wantarray_kind {
        WantarrayCtx::List => PerlValue::array(out),
        WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
        WantarrayCtx::Void => PerlValue::UNDEF,
    })
}

/// `with_index LIST` — each element is an arrayref `[$item, $index]` (0-based).
fn builtin_with_index(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let wa = interp.wantarray_kind;
    let mut out = Vec::with_capacity(args.len());
    for (i, item) in args.iter().cloned().enumerate() {
        out.push(PerlValue::array_ref(Arc::new(RwLock::new(vec![
            item,
            PerlValue::integer(i as i64),
        ]))));
    }
    Ok(match wa {
        WantarrayCtx::List => PerlValue::array(out),
        WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
        WantarrayCtx::Void => PerlValue::UNDEF,
    })
}

fn builtin_which(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let name = args
        .first()
        .filter(|v| !v.to_string().is_empty())
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("which needs a program name", line))?;
    let dot = args
        .get(1)
        .is_some_and(|o| opt_hash_bool(o, "dot") || opt_hash_bool(o, "cwd"));
    Ok(crate::perl_fs::which_executable(&name, dot)
        .map(PerlValue::string)
        .unwrap_or(PerlValue::UNDEF))
}

fn builtin_json_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args
        .first()
        .ok_or_else(|| PerlError::runtime("json_encode needs a value", 0))?;
    let s = crate::native_data::json_encode(v)?;
    Ok(PerlValue::string(s))
}

fn builtin_json_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::json_decode(&s)
}

fn builtin_json_jq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data = args
        .first()
        .ok_or_else(|| PerlError::runtime("json_jq needs (data, jq_filter)", 0))?;
    let filter = args
        .get(1)
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PerlError::runtime("json_jq needs a jq filter string", 0))?;
    crate::native_data::json_jq(data, filter.trim())
}

fn builtin_toml_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_codec::toml_decode(&s)
}

fn builtin_xml_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_codec::xml_decode(&s)
}

fn builtin_xml_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args
        .first()
        .ok_or_else(|| PerlError::runtime("xml_encode needs a value", 0))?;
    crate::native_codec::xml_encode(v)
}

fn builtin_toml_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args
        .first()
        .ok_or_else(|| PerlError::runtime("toml_encode needs a value", 0))?;
    crate::native_codec::toml_encode(v)
}

fn builtin_yaml_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_codec::yaml_decode(&s)
}

fn builtin_yaml_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args
        .first()
        .ok_or_else(|| PerlError::runtime("yaml_encode needs a value", 0))?;
    crate::native_codec::yaml_encode(v)
}

fn builtin_fetch_async(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let url = args.first().map(|v| v.to_string()).unwrap_or_default();
    let result_slot: Arc<Mutex<Option<PerlResult<PerlValue>>>> = Arc::new(Mutex::new(None));
    let rs = Arc::clone(&result_slot);
    let join_slot: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let j = Arc::clone(&join_slot);
    let h = std::thread::spawn(move || {
        let out = crate::native_data::fetch(&url);
        *rs.lock() = Some(out);
    });
    *j.lock() = Some(h);
    Ok(PerlValue::async_task(Arc::new(PerlAsyncTask {
        result: result_slot,
        join: join_slot,
    })))
}

fn builtin_fetch_async_json(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let url = args.first().map(|v| v.to_string()).unwrap_or_default();
    let result_slot: Arc<Mutex<Option<PerlResult<PerlValue>>>> = Arc::new(Mutex::new(None));
    let rs = Arc::clone(&result_slot);
    let join_slot: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let j = Arc::clone(&join_slot);
    let h = std::thread::spawn(move || {
        let out = crate::native_data::fetch_json(&url);
        *rs.lock() = Some(out);
    });
    *j.lock() = Some(h);
    Ok(PerlValue::async_task(Arc::new(PerlAsyncTask {
        result: result_slot,
        join: join_slot,
    })))
}

fn builtin_par_fetch(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut urls = Vec::new();
    for a in args {
        urls.extend(a.to_list());
    }
    let out: Vec<PerlValue> = urls
        .into_par_iter()
        .map(|u| crate::native_data::fetch(&u.to_string()).unwrap_or(PerlValue::UNDEF))
        .collect();
    Ok(PerlValue::array(out))
}

fn builtin_par_csv_read(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::par_csv_read(&path)
}

fn builtin_quotemeta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(perl_quotemeta(&s)))
}

fn builtin_prototype(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    Ok(args[0]
        .as_code_ref()
        .map(|sub| {
            if !sub.params.is_empty() {
                PerlValue::UNDEF
            } else {
                PerlValue::string(sub.prototype.clone().unwrap_or_default())
            }
        })
        .unwrap_or(PerlValue::UNDEF))
}

#[cfg(unix)]
fn builtin_fork() -> PerlResult<PerlValue> {
    let pid = unsafe { libc::fork() };
    Ok(PerlValue::integer(pid as i64))
}

#[cfg(not(unix))]
fn builtin_fork() -> PerlResult<PerlValue> {
    Err(PerlError::runtime(
        "fork is not available on this platform",
        0,
    ))
}

#[cfg(unix)]
fn builtin_wait() -> PerlResult<PerlValue> {
    let mut status: libc::c_int = 0;
    let pid = unsafe { libc::wait(&mut status) };
    Ok(PerlValue::integer(pid as i64))
}

#[cfg(not(unix))]
fn builtin_wait() -> PerlResult<PerlValue> {
    Err(PerlError::runtime(
        "wait is not available on this platform",
        0,
    ))
}

#[cfg(unix)]
fn builtin_waitpid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pid = args.first().map(|v| v.to_int()).unwrap_or(-1) as libc::pid_t;
    let flags = args.get(1).map(|v| v.to_int()).unwrap_or(0) as libc::c_int;
    let mut status: libc::c_int = 0;
    let r = unsafe { libc::waitpid(pid, &mut status, flags) };
    Ok(PerlValue::integer(r as i64))
}

#[cfg(not(unix))]
fn builtin_waitpid(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Err(PerlError::runtime(
        "waitpid is not available on this platform",
        0,
    ))
}

#[cfg(unix)]
fn builtin_kill(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Ok(PerlValue::integer(0));
    }
    let pid = args[0].to_int() as libc::pid_t;
    let sig = args[1].to_int() as libc::c_int;
    let r = unsafe { libc::kill(pid, sig) };
    Ok(PerlValue::integer(r as i64))
}

#[cfg(not(unix))]
fn builtin_kill(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(0))
}

fn builtin_alarm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sec = args.first().map(|v| v.to_int().max(0) as u32).unwrap_or(0);
    #[cfg(unix)]
    {
        let prev = unsafe { libc::alarm(sec) };
        Ok(PerlValue::integer(prev as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = sec;
        Ok(PerlValue::integer(0))
    }
}

fn builtin_sleep(args: &[PerlValue]) -> PerlResult<PerlValue> {
    // Stock Perl's `sleep` is signal-interruptible and returns the actual seconds slept. We
    // mirror that by sleeping in short chunks and bailing as soon as `SIGINT`/`SIGTERM`/`SIGALRM`
    // are pending — otherwise a `pfor { sleep N }` worker would ignore Ctrl-C for the full `N`.
    let secs = args.first().map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let total = Duration::from_secs_f64(secs);
    let start = Instant::now();
    let chunk = Duration::from_millis(100);
    while start.elapsed() < total {
        if crate::perl_signal::pending("INT")
            || crate::perl_signal::pending("TERM")
            || crate::perl_signal::pending("ALRM")
        {
            break;
        }
        let remaining = total - start.elapsed();
        std::thread::sleep(remaining.min(chunk));
    }
    Ok(PerlValue::integer(start.elapsed().as_secs() as i64))
}

fn builtin_times() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let mut tms: libc::tms = unsafe { std::mem::zeroed() };
        let _ = unsafe { libc::times(&mut tms) };
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
        let user = tms.tms_utime as f64 / hz;
        let system = tms.tms_stime as f64 / hz;
        let cuser = tms.tms_cutime as f64 / hz;
        let csystem = tms.tms_cstime as f64 / hz;
        Ok(PerlValue::array(vec![
            PerlValue::float(user),
            PerlValue::float(system),
            PerlValue::float(cuser),
            PerlValue::float(csystem),
        ]))
    }
    #[cfg(not(unix))]
    {
        Ok(PerlValue::array(vec![
            PerlValue::float(0.0),
            PerlValue::float(0.0),
            PerlValue::float(0.0),
            PerlValue::float(0.0),
        ]))
    }
}

fn builtin_time() -> PerlResult<PerlValue> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(PerlValue::integer(secs))
}

fn builtin_canonpath(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(crate::perl_fs::canonpath_logical(&path)))
}

type LocaltimeParts = (i64, i64, i64, i64, i64, i64, i64, i64, i64);

fn localtime_parts(secs: i64, utc: bool) -> Option<LocaltimeParts> {
    if utc {
        let dt = Utc.timestamp_opt(secs, 0).single()?;
        let wday = dt.weekday().num_days_from_sunday() as i64;
        Some((
            dt.second() as i64,
            dt.minute() as i64,
            dt.hour() as i64,
            dt.day() as i64,
            dt.month0() as i64,
            (dt.year() - 1900) as i64,
            wday,
            dt.ordinal0() as i64,
            -1,
        ))
    } else {
        let dt = Local.timestamp_opt(secs, 0).latest()?;
        let wday = dt.weekday().num_days_from_sunday() as i64;
        Some((
            dt.second() as i64,
            dt.minute() as i64,
            dt.hour() as i64,
            dt.day() as i64,
            dt.month0() as i64,
            (dt.year() - 1900) as i64,
            wday,
            dt.ordinal0() as i64,
            -1,
        ))
    }
}

fn localtime_scalar(secs: i64, utc: bool) -> String {
    if utc {
        if let Some(dt) = Utc.timestamp_opt(secs, 0).single() {
            return format!("{}\n", dt.format("%a %b %e %H:%M:%S %Y"));
        }
    } else if let Some(dt) = Local.timestamp_opt(secs, 0).latest() {
        return format!("{}\n", dt.format("%a %b %e %H:%M:%S %Y"));
    }
    "\n".to_string()
}

fn builtin_getlogin() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        unsafe {
            let p = libc::getlogin();
            if !p.is_null() {
                let s = CStr::from_ptr(p).to_string_lossy().into_owned();
                if !s.is_empty() {
                    return Ok(PerlValue::string(s));
                }
            }
        }
    }
    for key in ["LOGNAME", "USER"] {
        if let Ok(s) = std::env::var(key) {
            if !s.is_empty() {
                return Ok(PerlValue::string(s));
            }
        }
    }
    Ok(PerlValue::UNDEF)
}

fn builtin_getppid() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        Ok(PerlValue::integer(unsafe { libc::getppid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Ok(PerlValue::integer(-1))
    }
}

fn builtin_getpgrp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let pid = args.first().map(|a| a.to_int() as libc::pid_t).unwrap_or(0);
        let g = unsafe { libc::getpgid(pid) };
        if g < 0 {
            return Ok(PerlValue::UNDEF);
        }
        Ok(PerlValue::integer(g as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        Ok(PerlValue::UNDEF)
    }
}

fn builtin_setpgrp(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let r = match args.len() {
            0 => unsafe { libc::setpgid(0, 0) },
            2 => unsafe {
                libc::setpgid(
                    args[0].to_int() as libc::pid_t,
                    args[1].to_int() as libc::pid_t,
                )
            },
            _ => {
                return Err(PerlError::runtime(
                    "setpgrp: expected 0 or 2 arguments",
                    line,
                ));
            }
        };
        if r != 0 {
            return Ok(PerlValue::integer(0));
        }
        Ok(PerlValue::integer(1))
    }
    #[cfg(not(unix))]
    {
        let _ = (args, line);
        Err(PerlError::runtime(
            "setpgrp: not available on this platform",
            line,
        ))
    }
}

#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "watchos",
        target_os = "tvos"
    )
))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__error()
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "watchos",
        target_os = "tvos"
    ))
))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__errno_location()
}

fn builtin_getpriority(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("getpriority: need WHICH and WHO", line));
    }
    #[cfg(unix)]
    {
        let which = args[0].to_int() as libc::c_int;
        let who = args[1].to_int() as libc::id_t;
        unsafe {
            *errno_ptr() = 0;
        }
        let p = unsafe { libc::getpriority(which, who) };
        if p == -1 && unsafe { *errno_ptr() } != 0 {
            return Ok(PerlValue::UNDEF);
        }
        Ok(PerlValue::integer(p as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        Ok(PerlValue::UNDEF)
    }
}

fn builtin_setpriority(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 3 {
        return Err(PerlError::runtime(
            "setpriority: need WHICH, WHO, and PRIORITY",
            line,
        ));
    }
    #[cfg(unix)]
    {
        let which = args[0].to_int() as libc::c_int;
        let who = args[1].to_int() as libc::id_t;
        let prio = args[2].to_int() as libc::c_int;
        let r = unsafe { libc::setpriority(which, who, prio) };
        if r != 0 {
            return Ok(PerlValue::integer(0));
        }
        Ok(PerlValue::integer(1))
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        Ok(PerlValue::integer(0))
    }
}

#[cfg(unix)]
fn passwd_entry_list(pw: &libc::passwd) -> Vec<PerlValue> {
    let s = |p: *const libc::c_char| -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    };
    vec![
        PerlValue::string(s(pw.pw_name)),
        PerlValue::string(s(pw.pw_passwd)),
        PerlValue::integer(pw.pw_uid as i64),
        PerlValue::integer(pw.pw_gid as i64),
        PerlValue::string(String::new()),
        PerlValue::string(String::new()),
        PerlValue::string(s(pw.pw_gecos)),
        PerlValue::string(s(pw.pw_dir)),
        PerlValue::string(s(pw.pw_shell)),
    ]
}

#[cfg(unix)]
fn fetch_passwd_by_uid(uid: libc::uid_t) -> Option<libc::passwd> {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pw,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(pw)
}

#[cfg(unix)]
fn fetch_passwd_by_name(name: &str) -> Option<libc::passwd> {
    let cname = CString::new(name.as_bytes()).ok()?;
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        libc::getpwnam_r(
            cname.as_ptr(),
            &mut pw,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(pw)
}

#[cfg(unix)]
fn group_entry_list(gr: &libc::group) -> Vec<PerlValue> {
    let name = if gr.gr_name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(gr.gr_name) }
            .to_string_lossy()
            .into_owned()
    };
    let passwd = if gr.gr_passwd.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(gr.gr_passwd) }
            .to_string_lossy()
            .into_owned()
    };
    let mut members = Vec::new();
    if !gr.gr_mem.is_null() {
        let mut i = 0;
        loop {
            let p = unsafe { *gr.gr_mem.add(i) };
            if p.is_null() {
                break;
            }
            members.push(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned());
            i += 1;
        }
    }
    vec![
        PerlValue::string(name),
        PerlValue::string(passwd),
        PerlValue::integer(gr.gr_gid as i64),
        PerlValue::string(members.join(" ")),
    ]
}

#[cfg(unix)]
fn fetch_group_by_gid(gid: libc::gid_t) -> Option<libc::group> {
    let mut gr: libc::group = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::group = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        libc::getgrgid_r(
            gid,
            &mut gr,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(gr)
}

#[cfg(unix)]
fn fetch_group_by_name(name: &str) -> Option<libc::group> {
    let cname = CString::new(name.as_bytes()).ok()?;
    let mut gr: libc::group = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::group = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        libc::getgrnam_r(
            cname.as_ptr(),
            &mut gr,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(gr)
}

#[cfg(unix)]
fn sockopt_payload(v: &PerlValue) -> Vec<u8> {
    if let Some(b) = v.as_bytes_arc() {
        return b.as_ref().to_vec();
    }
    let n = v.to_int() as i32;
    n.to_ne_bytes().to_vec()
}

impl Interpreter {
    pub(crate) fn builtin_localtime(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let secs = match args.first() {
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Some(a) if a.is_undef() => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Some(a) => a.to_int(),
        };
        let list_ctx = matches!(self.wantarray_kind, WantarrayCtx::List);
        if list_ctx {
            let Some(parts) = localtime_parts(secs, false) else {
                return Ok(PerlValue::UNDEF);
            };
            let (s, m, h, d, mon, y, wd, yd, dst) = parts;
            return Ok(PerlValue::array(vec![
                PerlValue::integer(s),
                PerlValue::integer(m),
                PerlValue::integer(h),
                PerlValue::integer(d),
                PerlValue::integer(mon),
                PerlValue::integer(y),
                PerlValue::integer(wd),
                PerlValue::integer(yd),
                PerlValue::integer(dst),
            ]));
        }
        Ok(PerlValue::string(localtime_scalar(secs, false)))
    }

    pub(crate) fn builtin_gmtime(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let secs = match args.first() {
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Some(a) if a.is_undef() => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Some(a) => a.to_int(),
        };
        let list_ctx = matches!(self.wantarray_kind, WantarrayCtx::List);
        if list_ctx {
            let Some(parts) = localtime_parts(secs, true) else {
                return Ok(PerlValue::UNDEF);
            };
            let (s, m, h, d, mon, y, wd, yd, dst) = parts;
            return Ok(PerlValue::array(vec![
                PerlValue::integer(s),
                PerlValue::integer(m),
                PerlValue::integer(h),
                PerlValue::integer(d),
                PerlValue::integer(mon),
                PerlValue::integer(y),
                PerlValue::integer(wd),
                PerlValue::integer(yd),
                PerlValue::integer(dst),
            ]));
        }
        Ok(PerlValue::string(localtime_scalar(secs, true)))
    }

    pub(crate) fn builtin_getpwuid(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let uid = args
                .first()
                .ok_or_else(|| PerlError::runtime("getpwuid: need UID", line))?
                .to_int() as libc::uid_t;
            let Some(pw) = fetch_passwd_by_uid(uid) else {
                return Ok(PerlValue::UNDEF);
            };
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(passwd_entry_list(&pw)));
            }
            let name = if pw.pw_name.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(pw.pw_name) }
                    .to_string_lossy()
                    .into_owned()
            };
            Ok(PerlValue::string(name))
        }
    }

    pub(crate) fn builtin_getpwnam(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let name = args
                .first()
                .ok_or_else(|| PerlError::runtime("getpwnam: need NAME", line))?
                .to_string();
            let Some(pw) = fetch_passwd_by_name(&name) else {
                return Ok(PerlValue::UNDEF);
            };
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(passwd_entry_list(&pw)));
            }
            Ok(PerlValue::string(name))
        }
    }

    pub(crate) fn builtin_getgrgid(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let gid = args
                .first()
                .ok_or_else(|| PerlError::runtime("getgrgid: need GID", line))?
                .to_int() as libc::gid_t;
            let Some(gr) = fetch_group_by_gid(gid) else {
                return Ok(PerlValue::UNDEF);
            };
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(group_entry_list(&gr)));
            }
            let name = if gr.gr_name.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(gr.gr_name) }
                    .to_string_lossy()
                    .into_owned()
            };
            Ok(PerlValue::string(name))
        }
    }

    pub(crate) fn builtin_getgrnam(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let name = args
                .first()
                .ok_or_else(|| PerlError::runtime("getgrnam: need NAME", line))?
                .to_string();
            let Some(gr) = fetch_group_by_name(&name) else {
                return Ok(PerlValue::UNDEF);
            };
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(group_entry_list(&gr)));
            }
            Ok(PerlValue::string(name))
        }
    }

    pub(crate) fn builtin_gethostbyname(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        let host = args
            .first()
            .ok_or_else(|| PerlError::runtime("gethostbyname: need NAME", line))?
            .to_string();
        let mut addrs: Vec<SocketAddr> = match format!("{}:0", host.trim()).to_socket_addrs() {
            Ok(i) => i.collect(),
            Err(_) => return Ok(PerlValue::UNDEF),
        };
        if addrs.is_empty() {
            return Ok(PerlValue::UNDEF);
        }
        addrs.sort_by_key(|a| match a {
            SocketAddr::V4(_) => 0,
            SocketAddr::V6(_) => 1,
        });
        let list_ctx = matches!(self.wantarray_kind, WantarrayCtx::List);
        let mut packed: Vec<PerlValue> = Vec::new();
        for a in &addrs {
            if let SocketAddr::V4(v4) = a {
                let port = v4.port();
                let ip = v4.ip().octets();
                let mut blob = Vec::with_capacity(16);
                blob.extend_from_slice(&(libc::AF_INET as u16).to_ne_bytes());
                blob.extend_from_slice(&port.to_be_bytes());
                blob.extend_from_slice(&ip);
                blob.resize(16, 0);
                packed.push(PerlValue::bytes(Arc::new(blob)));
            }
        }
        if packed.is_empty() {
            return Ok(PerlValue::UNDEF);
        }
        if !list_ctx {
            return Ok(packed[0].clone());
        }
        let len = packed
            .first()
            .and_then(|v| v.as_bytes_arc())
            .map(|b| b.len() as i64)
            .unwrap_or(16);
        Ok(PerlValue::array(
            vec![
                PerlValue::string(host.clone()),
                PerlValue::string(String::new()),
                PerlValue::integer(libc::AF_INET as i64),
                PerlValue::integer(len),
            ]
            .into_iter()
            .chain(packed)
            .collect(),
        ))
    }

    pub(crate) fn builtin_getprotobyname(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let name = args
                .first()
                .ok_or_else(|| PerlError::runtime("getprotobyname: need NAME", line))?
                .to_string();
            let cname = CString::new(name.as_bytes())
                .map_err(|_| PerlError::runtime("getprotobyname: invalid name", line))?;
            let p = unsafe { libc::getprotobyname(cname.as_ptr()) };
            if p.is_null() {
                return Ok(PerlValue::UNDEF);
            }
            let proto = unsafe { (*p).p_proto as i64 };
            let pname = if unsafe { (*p).p_name }.is_null() {
                name.clone()
            } else {
                unsafe { CStr::from_ptr((*p).p_name) }
                    .to_string_lossy()
                    .into_owned()
            };
            // `protoent.p_aliases` layout is platform-specific; skip walking the alias list to
            // avoid misaligned / incompatible pointer chains (e.g. macOS vs Linux).
            let aliases = String::new();
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(vec![
                    PerlValue::string(pname),
                    PerlValue::string(aliases),
                    PerlValue::integer(proto),
                ]));
            }
            Ok(PerlValue::integer(proto))
        }
    }

    pub(crate) fn builtin_getservbyname(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Ok(PerlValue::UNDEF);
        }
        #[cfg(unix)]
        {
            let serv = args
                .first()
                .ok_or_else(|| PerlError::runtime("getservbyname: need SERVICE", line))?
                .to_string();
            let proto = args
                .get(1)
                .ok_or_else(|| PerlError::runtime("getservbyname: need PROTOCOL", line))?
                .to_string();
            let cs = CString::new(serv.as_bytes())
                .map_err(|_| PerlError::runtime("getservbyname: invalid service name", line))?;
            let cp = CString::new(proto.as_bytes())
                .map_err(|_| PerlError::runtime("getservbyname: invalid protocol", line))?;
            let se = unsafe { libc::getservbyname(cs.as_ptr(), cp.as_ptr()) };
            if se.is_null() {
                return Ok(PerlValue::UNDEF);
            }
            let port_host = u16::from_be(unsafe { (*se).s_port as u16 }) as i64;
            let sname = if unsafe { (*se).s_name }.is_null() {
                serv.clone()
            } else {
                unsafe { CStr::from_ptr((*se).s_name) }
                    .to_string_lossy()
                    .into_owned()
            };
            let aliases = String::new();
            let protonum = unsafe {
                let p = libc::getprotobyname(cp.as_ptr());
                if p.is_null() {
                    0
                } else {
                    (*p).p_proto as i64
                }
            };
            if matches!(self.wantarray_kind, WantarrayCtx::List) {
                return Ok(PerlValue::array(vec![
                    PerlValue::string(sname),
                    PerlValue::string(aliases),
                    PerlValue::integer(port_host),
                    PerlValue::integer(protonum),
                ]));
            }
            Ok(PerlValue::integer(port_host))
        }
    }

    #[cfg(unix)]
    fn socket_raw_fd(&self, fh: &str) -> Option<libc::c_int> {
        self.socket_handles.get(fh).map(|s| match s {
            PerlSocket::Stream(sock) => sock.as_raw_fd(),
            PerlSocket::Listener(l) => l.as_raw_fd(),
            PerlSocket::Udp(u) => u.as_raw_fd(),
        })
    }

    pub(crate) fn builtin_setsockopt(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Err(PerlError::runtime(
                "setsockopt: not available on this platform",
                line,
            ));
        }
        #[cfg(unix)]
        {
            if args.len() < 4 {
                return Err(PerlError::runtime(
                    "setsockopt: need SOCK, LEVEL, OPTNAME, OPTVAL",
                    line,
                ));
            }
            let fh = args[0].to_string();
            let level = args[1].to_int() as libc::c_int;
            let optname = args[2].to_int() as libc::c_int;
            let payload = sockopt_payload(&args[3]);
            let fd = self.socket_raw_fd(&fh).ok_or_else(|| {
                PerlError::runtime(format!("setsockopt: not a socket {}", fh), line)
            })?;
            let r = unsafe {
                libc::setsockopt(
                    fd,
                    level,
                    optname,
                    payload.as_ptr().cast(),
                    payload.len() as libc::socklen_t,
                )
            };
            if r != 0 {
                return Ok(PerlValue::integer(0));
            }
            Ok(PerlValue::integer(1))
        }
    }

    pub(crate) fn builtin_getsockopt(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Err(PerlError::runtime(
                "getsockopt: not available on this platform",
                line,
            ));
        }
        #[cfg(unix)]
        {
            if args.len() < 3 {
                return Err(PerlError::runtime(
                    "getsockopt: need SOCK, LEVEL, OPTNAME",
                    line,
                ));
            }
            let fh = args[0].to_string();
            let level = args[1].to_int() as libc::c_int;
            let optname = args[2].to_int() as libc::c_int;
            let fd = self.socket_raw_fd(&fh).ok_or_else(|| {
                PerlError::runtime(format!("getsockopt: not a socket {}", fh), line)
            })?;
            let mut buf = vec![0u8; 256];
            let mut len = buf.len() as libc::socklen_t;
            let r =
                unsafe { libc::getsockopt(fd, level, optname, buf.as_mut_ptr().cast(), &mut len) };
            if r != 0 {
                return Ok(PerlValue::UNDEF);
            }
            buf.truncate(len as usize);
            Ok(PerlValue::bytes(Arc::new(buf)))
        }
    }

    pub(crate) fn builtin_getpeername(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Err(PerlError::runtime(
                "getpeername: not available on this platform",
                line,
            ));
        }
        #[cfg(unix)]
        {
            let fh = args
                .first()
                .ok_or_else(|| PerlError::runtime("getpeername: need SOCK", line))?
                .to_string();
            let fd = self.socket_raw_fd(&fh).ok_or_else(|| {
                PerlError::runtime(format!("getpeername: not a socket {}", fh), line)
            })?;
            let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            let r = unsafe {
                libc::getpeername(
                    fd,
                    (&mut storage as *mut libc::sockaddr_storage).cast(),
                    &mut len,
                )
            };
            if r != 0 {
                return Ok(PerlValue::UNDEF);
            }
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    (&storage as *const libc::sockaddr_storage).cast(),
                    len as usize,
                )
            };
            Ok(PerlValue::bytes(Arc::new(bytes.to_vec())))
        }
    }

    pub(crate) fn builtin_getsockname(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(not(unix))]
        {
            let _ = (args, line, self);
            return Err(PerlError::runtime(
                "getsockname: not available on this platform",
                line,
            ));
        }
        #[cfg(unix)]
        {
            let fh = args
                .first()
                .ok_or_else(|| PerlError::runtime("getsockname: need SOCK", line))?
                .to_string();
            let fd = self.socket_raw_fd(&fh).ok_or_else(|| {
                PerlError::runtime(format!("getsockname: not a socket {}", fh), line)
            })?;
            let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            let r = unsafe {
                libc::getsockname(
                    fd,
                    (&mut storage as *mut libc::sockaddr_storage).cast(),
                    &mut len,
                )
            };
            if r != 0 {
                return Ok(PerlValue::UNDEF);
            }
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    (&storage as *const libc::sockaddr_storage).cast(),
                    len as usize,
                )
            };
            Ok(PerlValue::bytes(Arc::new(bytes.to_vec())))
        }
    }

    fn builtin_binmode(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let _ = (args, line);
        // Layer selection (`:utf8`) is a no-op; real binmode is platform-specific.
        Ok(PerlValue::integer(1))
    }

    fn builtin_fileno(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        #[cfg(unix)]
        {
            if let Some(f) = self.io_file_slots.get(&name) {
                return Ok(PerlValue::integer(f.lock().as_raw_fd() as i64));
            }
            match name.as_str() {
                "STDIN" => Ok(PerlValue::integer(0)),
                "STDOUT" => Ok(PerlValue::integer(1)),
                "STDERR" => Ok(PerlValue::integer(2)),
                _ => Ok(PerlValue::integer(-1)),
            }
        }
        #[cfg(not(unix))]
        {
            match name.as_str() {
                "STDIN" | "STDOUT" | "STDERR" => Ok(PerlValue::integer(0)),
                _ => Ok(PerlValue::integer(-1)),
            }
        }
    }

    /// `tell FILEHANDLE` / `tell` — byte offset for handles in [`Interpreter::io_file_slots`]
    /// (same underlying `File` as `sysseek`). Unseekable or unopened handles return `-1`.
    /// No-arg form uses [`Interpreter::last_readline_handle`] after `readline` / `<>` (Perl semantics).
    fn builtin_tell(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let name = match args.len() {
            0 => {
                if self.last_readline_handle.is_empty() {
                    return Ok(PerlValue::integer(-1));
                }
                self.last_readline_handle.clone()
            }
            1 => args[0]
                .as_io_handle_name()
                .unwrap_or_else(|| args[0].to_string()),
            _ => return Err(PerlError::runtime("tell: too many arguments", line)),
        };
        if let Some(slot) = self.io_file_slots.get(&name).cloned() {
            match slot.lock().seek(SeekFrom::Current(0)) {
                Ok(p) => Ok(PerlValue::integer(p as i64)),
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::integer(-1))
                }
            }
        } else {
            Ok(PerlValue::integer(-1))
        }
    }

    fn builtin_flock(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        let op = args.get(1).map(|v| v.to_int()).unwrap_or(0);
        #[cfg(unix)]
        {
            if let Some(f) = self.io_file_slots.get(&name) {
                let fd = f.lock().as_raw_fd();
                let lock_op = match op {
                    1 => libc::LOCK_SH,
                    2 => libc::LOCK_EX,
                    4 => libc::LOCK_NB | libc::LOCK_EX,
                    8 => libc::LOCK_UN,
                    _ => libc::LOCK_EX,
                };
                let r = unsafe { libc::flock(fd, lock_op) };
                return Ok(PerlValue::integer(if r == 0 { 1 } else { 0 }));
            }
        }
        let _ = line;
        Ok(PerlValue::integer(1))
    }

    fn builtin_getc(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let name = args
            .first()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "STDIN".to_string());
        let mut buf = [0u8; 1];
        if name == "STDIN" {
            match std::io::stdin().read(&mut buf) {
                Ok(0) => return Ok(PerlValue::UNDEF),
                Ok(_) => {
                    return Ok(PerlValue::string(decode_utf8_or_latin1(&buf[..1])));
                }
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    return Ok(PerlValue::UNDEF);
                }
            }
        }
        if let Some(slot) = self.io_file_slots.get(&name).cloned() {
            match slot.lock().read(&mut buf) {
                Ok(0) => Ok(PerlValue::UNDEF),
                Ok(_) => Ok(PerlValue::string(decode_utf8_or_latin1(&buf[..1]))),
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::UNDEF)
                }
            }
        } else {
            Err(PerlError::runtime(
                format!("getc: unopened handle {}", name),
                line,
            ))
        }
    }

    fn builtin_sysread(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("sysread: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let len = args[2].to_int().max(0) as usize;
        let offset = args.get(3).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
        let mut buf = vec![0u8; len];
        let n = if let Some(slot) = self.io_file_slots.get(&fh) {
            let mut f = slot.lock();
            if offset > 0 {
                let _ = f.seek(SeekFrom::Start(offset as u64));
            }
            f.read(&mut buf).unwrap_or(0)
        } else {
            return Err(PerlError::runtime(
                format!("sysread: unopened handle {}", fh),
                line,
            ));
        };
        Ok(PerlValue::integer(n as i64))
    }

    fn builtin_syswrite(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("syswrite: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let data = args[1].to_string();
        let len = args[2].to_int().max(0) as usize;
        let chunk = &data.as_bytes()[..len.min(data.len())];
        if let Some(slot) = self.io_file_slots.get(&fh) {
            let mut f = slot.lock();
            let n = f.write(chunk).unwrap_or(0);
            let _ = f.flush();
            return Ok(PerlValue::integer(n as i64));
        }
        Err(PerlError::runtime(
            format!("syswrite: unopened handle {}", fh),
            line,
        ))
    }

    fn builtin_sysseek(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("sysseek: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let pos = args[1].to_int();
        let whence = args[2].to_int();
        if let Some(slot) = self.io_file_slots.get(&fh).cloned() {
            let w = match whence {
                0 => SeekFrom::Start(pos as u64),
                1 => SeekFrom::Current(pos),
                2 => SeekFrom::End(pos),
                _ => SeekFrom::Start(pos as u64),
            };
            match slot.lock().seek(w) {
                Ok(p) => Ok(PerlValue::integer(p as i64)),
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::integer(-1))
                }
            }
        } else {
            Err(PerlError::runtime(
                format!("sysseek: unopened handle {}", fh),
                line,
            ))
        }
    }

    fn builtin_truncate(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("truncate: not enough arguments", line));
        }
        let path = args[0].to_string();
        let len = args[1].to_int().max(0) as u64;
        match std::fs::OpenOptions::new().write(true).open(&path) {
            Ok(f) => match f.set_len(len) {
                Ok(()) => Ok(PerlValue::integer(1)),
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::integer(0))
                }
            },
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::integer(0))
            }
        }
    }

    fn builtin_select(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        // Four-arg select(RB, WB, EB, timeout): sleep for timeout seconds (best-effort).
        if args.len() >= 4 {
            let t = args[3].to_number().max(0.0);
            std::thread::sleep(Duration::from_secs_f64(t));
            return Ok(PerlValue::integer(0));
        }
        // One-arg: set default output handle for print/say/printf; return previous handle name.
        if args.len() == 1 {
            let new = self.resolve_io_handle_name(&args[0].to_string());
            let old = std::mem::replace(&mut self.default_print_handle, new);
            return Ok(PerlValue::string(old));
        }
        Ok(PerlValue::integer(0))
    }

    fn builtin_socket(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 4 {
            return Err(PerlError::runtime(
                "socket: need handle, domain, type, protocol",
                line,
            ));
        }
        let fh = args[0].to_string();
        let typ = args[2].to_int();
        // SOCK_STREAM = 1, SOCK_DGRAM = 2 (common on Unix; best-effort)
        let res: Result<PerlSocket, String> = if typ == 2 {
            UdpSocket::bind("0.0.0.0:0")
                .map(PerlSocket::Udp)
                .map_err(|e| e.to_string())
        } else {
            TcpListener::bind("0.0.0.0:0")
                .map(PerlSocket::Listener)
                .map_err(|e| e.to_string())
        };
        match res {
            Ok(s) => {
                self.socket_handles.insert(fh, s);
                Ok(PerlValue::integer(1))
            }
            Err(e) => {
                self.errno = e;
                self.errno_code = 0;
                Ok(PerlValue::integer(0))
            }
        }
    }

    fn builtin_bind(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("bind: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let addr = args[1].to_string();
        // Replace listener with one bound to `addr` (host:port or :port).
        let sock = TcpListener::bind(addr.trim()).map(PerlSocket::Listener);
        match sock {
            Ok(s) => {
                self.socket_handles.insert(fh, s);
                Ok(PerlValue::integer(1))
            }
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::integer(0))
            }
        }
    }

    fn builtin_listen(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("listen: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let _backlog = args[1].to_int().max(1) as i32;
        if let Some(PerlSocket::Listener(_lis)) = self.socket_handles.get(&fh) {
            // `std::net::TcpListener` is already listening after bind.
            return Ok(PerlValue::integer(1));
        }
        Err(PerlError::runtime("listen: not a listener socket", line))
    }

    fn builtin_accept(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("accept: not enough arguments", line));
        }
        let new_fh = args[0].to_string();
        let srv = args[1].to_string();
        if let Some(PerlSocket::Listener(lis)) = self.socket_handles.get(&srv) {
            match lis.accept() {
                Ok((stream, _addr)) => {
                    self.socket_handles
                        .insert(new_fh, PerlSocket::Stream(stream));
                    Ok(PerlValue::integer(1))
                }
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::integer(0))
                }
            }
        } else {
            Err(PerlError::runtime("accept: bad listener", line))
        }
    }

    fn builtin_connect(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("connect: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let addr = args[1].to_string();
        match TcpStream::connect(addr.trim()) {
            Ok(s) => {
                self.socket_handles.insert(fh, PerlSocket::Stream(s));
                Ok(PerlValue::integer(1))
            }
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::integer(0))
            }
        }
    }

    fn builtin_send(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("send: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let data = args[1].to_string();
        if let Some(PerlSocket::Stream(s)) = self.socket_handles.get_mut(&fh) {
            let n = s.write(data.as_bytes()).unwrap_or(0);
            return Ok(PerlValue::integer(n as i64));
        }
        Err(PerlError::runtime("send: not a connected socket", line))
    }

    fn builtin_recv(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("recv: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let len = args[1].to_int().max(0) as usize;
        let mut buf = vec![0u8; len];
        if let Some(PerlSocket::Stream(s)) = self.socket_handles.get_mut(&fh) {
            let n = s.read(&mut buf).unwrap_or(0);
            return Ok(PerlValue::string(decode_utf8_or_latin1(&buf[..n])));
        }
        Err(PerlError::runtime("recv: not a connected socket", line))
    }

    fn builtin_shutdown(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime("shutdown: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let how = args[1].to_int();
        let sh = match how {
            0 => Shutdown::Read,
            1 => Shutdown::Write,
            _ => Shutdown::Both,
        };
        if let Some(PerlSocket::Stream(s)) = self.socket_handles.get_mut(&fh) {
            let _ = s.shutdown(sh);
            return Ok(PerlValue::integer(1));
        }
        Err(PerlError::runtime("shutdown: not a stream socket", line))
    }
}
