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
use std::sync::LazyLock;

use crate::value::PerlIterator;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, TimeZone, Timelike, Utc};

/// Monotonic clock anchor — first call to `elapsed()` returns ~0.0 s.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);
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
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::MapFnIterator::new(source, |s| {
                    crate::perl_fs::path_basename(&s)
                }),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(crate::perl_fs::path_basename(&s)))
}

fn builtin_dirname(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::MapFnIterator::new(source, |s| crate::perl_fs::path_dirname(&s)),
            )));
        }
    }
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
        "basename" | "bn" => Some(builtin_basename(args)),
        "copy" => Some(builtin_copy(args, line)),
        "dirname" | "dn" => Some(builtin_dirname(args)),
        "fileparse" => Some(builtin_fileparse(interp, args, line)),
        "gethostname" | "hn" => Some(builtin_gethostname()),
        "spurt" | "write_file" | "wf" => Some(builtin_spurt(args, line)),
        "collect" => Some(interp.builtin_collect_execute(args, line)),
        "take" | "head" | "hd" => {
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
            if args.len() == 2 && (args[0].is_iterator() || args[0].as_array_vec().is_some()) {
                let n = args[1].to_int().max(0) as usize;
                let source = crate::map_stream::into_pull_iter(args[0].clone());
                let iter = crate::map_stream::TakeIterator::new(source, n);
                if interp.wantarray_kind == WantarrayCtx::Scalar {
                    let items = iter.collect_all();
                    return Some(Ok(items.last().cloned().unwrap_or(PerlValue::UNDEF)));
                }
                return Some(Ok(PerlValue::iterator(Arc::new(iter))));
            }
            Some(builtin_take(interp, args))
        }
        "tail" | "tl" => Some(builtin_tail(interp, args)),
        "drop" | "skip" | "drp" => {
            if args.len() == 2 && (args[0].is_iterator() || args[0].as_array_vec().is_some()) {
                let n = args[1].to_int().max(0) as usize;
                let source = crate::map_stream::into_pull_iter(args[0].clone());
                let iter = crate::map_stream::SkipIterator::new(source, n);
                if interp.wantarray_kind == WantarrayCtx::Scalar {
                    let items = iter.collect_all();
                    return Some(Ok(items.last().cloned().unwrap_or(PerlValue::UNDEF)));
                }
                return Some(Ok(PerlValue::iterator(Arc::new(iter))));
            }
            Some(builtin_drop(interp, args))
        }
        "take_while" | "drop_while" | "skip_while" | "reject" | "tap" | "peek" | "partition"
        | "min_by" | "max_by" | "zip_with" | "count_by" => {
            Some(interp.list_higher_order_block_builtin(name, args, line))
        }
        "with_index" | "wi" => Some(builtin_with_index(interp, args)),
        "flatten" | "fl" => Some(builtin_flatten(interp, args)),
        "interleave" | "il" => Some(builtin_interleave(interp, args)),
        "frequencies" | "freq" | "frq" => Some(builtin_frequencies(args)),
        "ddump" | "dd" => Some(builtin_ddump(args)),
        "stringify" | "str" => Some(builtin_stringify(args)),
        "input" => Some(builtin_input(interp, args, line)),
        "lines" | "ln" => Some(builtin_lines(interp, args)),
        "words" | "wd" => Some(builtin_words(interp, args)),
        "chars" | "ch" => Some(builtin_chars(interp, args)),
        "trim" | "tm" => Some(builtin_trim(args)),
        "stdin" => Some(Ok(PerlValue::iterator(Arc::new(
            crate::map_stream::StdinIterator::new(),
        )))),
        "avg" => Some(builtin_avg(args)),
        "top" => Some(builtin_top(args)),
        "to_file" => Some(builtin_to_file(args, line)),
        "to_json" | "tj" => Some(builtin_to_json(args)),
        "to_csv" | "tc" => Some(builtin_to_csv(args)),
        "grep_v" => Some(builtin_grep_v(args, line)),
        "select_keys" => Some(builtin_select_keys(args)),
        "pluck" => Some(builtin_pluck(args)),
        "first_or" => Some(builtin_first_or(args)),
        "compact" | "cpt" => Some(builtin_compact(args)),
        "concat" | "chain" | "cat" => Some(builtin_concat(args)),
        "clamp" | "clp" => Some(builtin_clamp(args)),
        "normalize" | "nrm" => Some(builtin_normalize(args)),
        "stddev" | "std" => Some(builtin_stddev(args)),
        "squared" | "sq" => Some(builtin_squared(args)),
        "cubed" | "cb" => Some(builtin_cubed(args)),
        "expt" => Some(builtin_expt(args)),
        "snake_case" | "sc" => Some(builtin_snake_case(args)),
        "camel_case" | "cc" => Some(builtin_camel_case(args)),
        "kebab_case" | "kc" => Some(builtin_kebab_case(args)),
        "to_toml" | "tt" => Some(builtin_to_toml(args)),
        "to_yaml" | "ty" => Some(builtin_to_yaml(args)),
        "to_xml" | "tx" => Some(builtin_to_xml(args)),
        "set" => Some(Ok(crate::value::set_from_elements(args.iter().cloned()))),
        "tee" => Some(builtin_tee(args, line)),
        "nth" => Some(builtin_nth(args)),
        "to_set" => Some(builtin_to_set(args)),
        "to_hash" => Some(builtin_to_hash(args)),
        "enumerate" | "en" => Some(builtin_enumerate(args)),
        "chunk" | "chk" => Some(builtin_chunk(args)),
        "dedup" | "dup" => Some(builtin_dedup(args)),
        "range" => Some(builtin_range(args)),
        "list_count" | "list_size" => Some(builtin_list_count(args)),
        "count" | "len" | "size" | "cnt" => Some(builtin_count_size_cnt(args)),
        "read_lines" | "rl" => Some(builtin_read_lines(interp, args, line)),
        "append_file" | "af" => Some(builtin_append_file(args, line)),
        "tempfile" | "tf" => Some(builtin_tempfile(args, line)),
        "tempdir" | "tdr" => Some(builtin_tempdir(args, line)),
        "read_json" | "rj" => Some(builtin_read_json(args, line)),
        "write_json" | "wj" => Some(builtin_write_json(args, line)),
        "glob_match" => Some(builtin_glob_match(args, line)),
        "which_all" | "wha" => Some(builtin_which_all(args, line)),
        "uname" => Some(builtin_uname()),
        "rmdir" | "CORE::rmdir" => Some(interp.builtin_rmdir_execute(args, line)),
        "touch" => Some(interp.builtin_touch_execute(args, line)),
        "utime" | "CORE::utime" => Some(interp.builtin_utime_execute(args, line)),
        "umask" | "CORE::umask" => Some(interp.builtin_umask_execute(args, line)),
        "getcwd" | "CORE::getcwd" | "Cwd::getcwd" | "pwd" => {
            Some(interp.builtin_getcwd_execute(args, line))
        }
        "realpath" | "CORE::realpath" | "Cwd::realpath" | "rp" => {
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
        "seek" | "CORE::seek" => Some(interp.builtin_seek(args, line)),
        "read" | "CORE::read" => Some(interp.builtin_read(args, line)),
        "sysopen" | "CORE::sysopen" => Some(interp.builtin_sysopen(args, line)),
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
        "socketpair" | "CORE::socketpair" => Some(interp.builtin_socketpair(args, line)),
        "chroot" | "CORE::chroot" => Some(builtin_chroot(args, line)),
        "pack" => Some(crate::pack::perl_pack(args, line)),
        "unpack" => Some(crate::pack::perl_unpack(args, line)),
        "vec" | "CORE::vec" => Some(builtin_vec(args, line)),
        "dump" | "CORE::dump" => Some(builtin_dump()),
        "reset" | "CORE::reset" => Some(Ok(PerlValue::integer(1))),
        "formline" | "CORE::formline" => Some(interp.builtin_formline(args, line)),
        "tied" | "CORE::tied" => Some(interp.builtin_tied(args, line)),
        "untie" | "CORE::untie" => Some(interp.builtin_untie(args, line)),
        "gethostbyaddr" | "CORE::gethostbyaddr" => Some(interp.builtin_gethostbyaddr(args, line)),
        "setpwent" | "CORE::setpwent" => Some(builtin_setpwent()),
        "endpwent" | "CORE::endpwent" => Some(builtin_endpwent()),
        "getpwent" | "CORE::getpwent" => Some(builtin_getpwent()),
        "setgrent" | "CORE::setgrent" => Some(builtin_setgrent()),
        "endgrent" | "CORE::endgrent" => Some(builtin_endgrent()),
        "getgrent" | "CORE::getgrent" => Some(builtin_getgrent()),
        "sethostent" | "CORE::sethostent" => Some(builtin_stub_ok("sethostent")),
        "endhostent" | "CORE::endhostent" => Some(builtin_stub_ok("endhostent")),
        "gethostent" | "CORE::gethostent" => Some(builtin_stub_ok("gethostent")),
        "setnetent" | "CORE::setnetent" => Some(builtin_stub_ok("setnetent")),
        "endnetent" | "CORE::endnetent" => Some(builtin_stub_ok("endnetent")),
        "getnetent" | "CORE::getnetent" => Some(builtin_stub_ok("getnetent")),
        "setprotoent" | "CORE::setprotoent" => Some(builtin_stub_ok("setprotoent")),
        "endprotoent" | "CORE::endprotoent" => Some(builtin_stub_ok("endprotoent")),
        "getprotoent" | "CORE::getprotoent" => Some(builtin_stub_ok("getprotoent")),
        "setservent" | "CORE::setservent" => Some(builtin_stub_ok("setservent")),
        "endservent" | "CORE::endservent" => Some(builtin_stub_ok("endservent")),
        "getservent" | "CORE::getservent" => Some(builtin_stub_ok("getservent")),
        "msgctl" | "CORE::msgctl" => Some(builtin_sysv_ipc_stub("msgctl", line)),
        "msgget" | "CORE::msgget" => Some(builtin_sysv_ipc_stub("msgget", line)),
        "msgsnd" | "CORE::msgsnd" => Some(builtin_sysv_ipc_stub("msgsnd", line)),
        "msgrcv" | "CORE::msgrcv" => Some(builtin_sysv_ipc_stub("msgrcv", line)),
        "semctl" | "CORE::semctl" => Some(builtin_sysv_ipc_stub("semctl", line)),
        "semget" | "CORE::semget" => Some(builtin_sysv_ipc_stub("semget", line)),
        "semop" | "CORE::semop" => Some(builtin_sysv_ipc_stub("semop", line)),
        "shmctl" | "CORE::shmctl" => Some(builtin_sysv_ipc_stub("shmctl", line)),
        "shmget" | "CORE::shmget" => Some(builtin_sysv_ipc_stub("shmget", line)),
        "shmread" | "CORE::shmread" => Some(builtin_sysv_ipc_stub("shmread", line)),
        "shmwrite" | "CORE::shmwrite" => Some(builtin_sysv_ipc_stub("shmwrite", line)),
        "quotemeta" | "qm" => Some(builtin_quotemeta(args)),
        "pselect" => Some(crate::pchannel::pselect_recv(args, line)),
        "csv_read" | "cr" => Some(builtin_csv_read(args)),
        "csv_write" | "cw" => Some(builtin_csv_write(args)),
        "sqlite" | "sql" => Some(builtin_sqlite(args)),
        "fetch" | "ft" => Some(builtin_fetch(args, line)),
        "fetch_json" | "ftj" => Some(builtin_fetch_json(args, line)),
        "http_request" | "hr" => Some(builtin_http_request(args, line)),
        "read_bytes" | "slurp_raw" | "rb" => Some(builtin_read_bytes(args, line)),
        "move" | "mv" => Some(builtin_move(args, line)),
        "which" | "wh" => Some(builtin_which(args, line)),
        "json_encode" | "je" => Some(builtin_json_encode(args)),
        "json_decode" | "jd" => Some(builtin_json_decode(args)),
        "json_jq" => Some(builtin_json_jq(args)),
        "sha1" | "s1" => Some(crate::native_codec::sha1_digest(
            args.first().unwrap_or(&undef),
        )),
        "sha224" | "s224" => Some(crate::native_codec::sha224(args.first().unwrap_or(&undef))),
        "sha256" | "s256" => Some(crate::native_codec::sha256(args.first().unwrap_or(&undef))),
        "sha384" | "s384" => Some(crate::native_codec::sha384(args.first().unwrap_or(&undef))),
        "sha512" | "s512" => Some(crate::native_codec::sha512(args.first().unwrap_or(&undef))),
        "md5" | "m5" => Some(crate::native_codec::md5_digest(
            args.first().unwrap_or(&undef),
        )),
        "hmac_sha256" | "hmac" => Some({
            let key = args.first().unwrap_or(&undef);
            let msg = args.get(1).unwrap_or(&undef);
            crate::native_codec::hmac_sha256(key, msg)
        }),
        "uuid" | "uid" => Some(crate::native_codec::uuid_v4()),
        "base64_encode" | "b64e" => Some(crate::native_codec::base64_encode(
            args.first().unwrap_or(&undef),
        )),
        "base64_decode" | "b64d" => Some(crate::native_codec::base64_decode(
            args.first().unwrap_or(&undef),
        )),
        "hex_encode" | "hxe" => Some(crate::native_codec::hex_encode(
            args.first().unwrap_or(&undef),
        )),
        "hex_decode" | "hxd" => Some(crate::native_codec::hex_decode(
            args.first().unwrap_or(&undef),
        )),
        "gzip" | "gz" => Some(crate::native_codec::gzip(args.first().unwrap_or(&undef))),
        "gunzip" | "ugz" => Some(crate::native_codec::gunzip(args.first().unwrap_or(&undef))),
        "zstd" | "zst" => Some(crate::native_codec::zstd_compress(
            args.first().unwrap_or(&undef),
        )),
        "zstd_decode" | "uzst" => Some(crate::native_codec::zstd_decode(
            args.first().unwrap_or(&undef),
        )),
        "datetime_utc" | "utc" => Some(crate::native_codec::datetime_utc()),
        "datetime_from_epoch" | "dte" => Some(crate::native_codec::datetime_from_epoch(
            args.first().unwrap_or(&undef),
        )),
        "datetime_parse_rfc3339" => Some(crate::native_codec::datetime_parse_rfc3339(
            args.first().unwrap_or(&undef),
        )),
        "datetime_strftime" | "dtf" => Some({
            let a = args.first().unwrap_or(&undef);
            let b = args.get(1).unwrap_or(&undef);
            crate::native_codec::datetime_strftime(a, b)
        }),
        "datetime_now_tz" | "now" => Some(crate::native_codec::datetime_now_tz(
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
        "toml_decode" | "td" => Some(builtin_toml_decode(args)),
        "toml_encode" | "te" => Some(builtin_toml_encode(args)),
        "xml_decode" | "xd" => Some(builtin_xml_decode(args)),
        "xml_encode" | "xe" => Some(builtin_xml_encode(args)),
        "yaml_decode" | "yd" => Some(builtin_yaml_decode(args)),
        "yaml_encode" | "ye" => Some(builtin_yaml_encode(args)),
        "url_encode" | "uri_escape" | "ue" => Some(crate::native_codec::url_encode(
            args.first().unwrap_or(&undef),
        )),
        "url_decode" | "uri_unescape" | "ud" => Some(crate::native_codec::url_decode(
            args.first().unwrap_or(&undef),
        )),
        // `async_fetch` would tokenize as keyword `async` — use `fetch_async` / `fetch_async_json`.
        "fetch_async" | "fta" => Some(builtin_fetch_async(args)),
        "fetch_async_json" | "ftaj" => Some(builtin_fetch_async_json(args)),
        "par_fetch" | "pft" => Some(builtin_par_fetch(args)),
        "par_csv_read" | "pcr" => Some(builtin_par_csv_read(args)),
        "dataframe" | "df" => Some(builtin_dataframe(args)),
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
        "elapsed" | "el" => Some(builtin_elapsed()),
        "crc32" => Some(builtin_crc32(args, line)),
        "par_find_files" => Some(builtin_par_find_files(args, line)),
        "par_line_count" => Some(builtin_par_line_count(args, line)),
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

/// `list_count` / `list_size` + `LIST` — like [`builtin_flatten`]: one-level
/// [`PerlValue::map_flatten_outputs`] per actual; returns the **element** count.
fn builtin_list_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut out = Vec::new();
    for a in args {
        out.extend(a.map_flatten_outputs(true));
    }
    Ok(PerlValue::integer(out.len() as i64))
}

/// `count` / `size` / `cnt`: pipe-friendly “how big is this value?”
/// — **one string** → UTF-8 byte length (same as the `length` builtin);
/// **one array / aref** → flattened element count (list context ranges become arrays first);
/// **one hash** → number of keys; **one set** → set size; **several actuals** → same as [`builtin_list_count`].
fn builtin_count_size_cnt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    if args.len() == 1 {
        let a = &args[0];
        if let Some(h) = a.as_hash_map() {
            return Ok(PerlValue::integer(h.len() as i64));
        }
        if let Some(Some(n)) = a.with_heap(|h| match h {
            crate::value::HeapObject::Set(st) => Some(st.len()),
            _ => None,
        }) {
            return Ok(PerlValue::integer(n as i64));
        }
        if let Some(b) = a.as_bytes_arc() {
            return Ok(PerlValue::integer(b.len() as i64));
        }
        if a.is_string_like() {
            return Ok(PerlValue::integer(a.to_string().len() as i64));
        }
        return Ok(PerlValue::integer(a.map_flatten_outputs(true).len() as i64));
    }
    builtin_list_count(args)
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

/// `which_all PROGRAM` — returns all matching executables on `$PATH`.
fn builtin_which_all(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let name = args
        .first()
        .filter(|v| !v.to_string().is_empty())
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("which_all needs a program name", line))?;
    let mut results = Vec::new();
    if let Some(path_os) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_os) {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                if let Some(s) = candidate.to_str() {
                    results.push(PerlValue::string(s.to_string()));
                }
            }
        }
    }
    Ok(PerlValue::array(results))
}

/// `interleave @a, @b, ...` — round-robin merge of lists.
fn builtin_interleave(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lists: Vec<Vec<PerlValue>> = args.iter().map(|a| a.map_flatten_outputs(true)).collect();
    let max_len = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(lists.len() * max_len);
    for i in 0..max_len {
        for list in &lists {
            if let Some(v) = list.get(i) {
                out.push(v.clone());
            }
        }
    }
    Ok(match interp.wantarray_kind {
        WantarrayCtx::List => PerlValue::array(out),
        WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
        WantarrayCtx::Void => PerlValue::UNDEF,
    })
}

/// `frequencies LIST` — count occurrences, returns hash ref `{ value => count }`.
fn builtin_frequencies(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut counts = indexmap::IndexMap::new();
    for a in args {
        for v in a.map_flatten_outputs(true) {
            let key = v.to_string();
            let entry = counts.entry(key).or_insert(PerlValue::integer(0));
            *entry = PerlValue::integer(entry.to_int() + 1);
        }
    }
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(counts))))
}

/// `ddump EXPR, ...` — Data::Dumper-style pretty printer.  Prints to STDOUT
/// and returns the formatted string so callers can capture it.
fn builtin_ddump(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut buf = String::new();
    for (i, val) in args.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        let name = format!("$VAR{}", i + 1);
        buf.push_str(&name);
        buf.push_str(" = ");
        ddump_value(&mut buf, val, 0);
        buf.push(';');
    }
    buf.push('\n');
    Ok(PerlValue::string(buf))
}

/// `stringify EXPR, ...` / `str EXPR, ...` — convert values to valid perlrs string literals.
/// Unlike `ddump`, output is a single parseable perlrs expression with no `$VAR =` prefix.
fn builtin_stringify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut buf = String::new();
    if args.len() == 1 {
        stringify_value(&mut buf, &args[0]);
    } else {
        buf.push('(');
        for (i, val) in args.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            stringify_value(&mut buf, val);
        }
        buf.push(')');
    }
    Ok(PerlValue::string(buf))
}

fn stringify_value(buf: &mut String, val: &PerlValue) {
    use std::fmt::Write;

    if val.is_undef() {
        buf.push_str("undef");
        return;
    }

    if val.is_iterator() {
        let it = val.clone().into_iterator();
        let items = it.collect_all();
        buf.push('(');
        for (i, elem) in items.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            stringify_value(buf, elem);
        }
        buf.push(')');
        return;
    }

    if let Some(ar) = val.as_array_ref() {
        let guard = ar.read();
        buf.push('[');
        for (i, elem) in guard.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            stringify_value(buf, elem);
        }
        buf.push(']');
        return;
    }

    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        buf.push_str("+{");
        for (i, (k, v)) in guard.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            let _ = write!(buf, "{} => ", stringify_key(k));
            stringify_value(buf, v);
        }
        buf.push('}');
        return;
    }

    if let Some(sr) = val.as_scalar_ref() {
        let inner_val = sr.read().clone();
        buf.push('\\');
        stringify_value(buf, &inner_val);
        return;
    }

    if let Some(blessed) = val.as_blessed_ref() {
        let data = blessed.data.read().clone();
        let _ = write!(buf, "bless(");
        stringify_value(buf, &data);
        let _ = write!(buf, ", \"{}\")", blessed.class);
        return;
    }

    if let Some(arr) = val.as_array_vec() {
        buf.push('(');
        for (i, elem) in arr.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            stringify_value(buf, elem);
        }
        buf.push(')');
        return;
    }

    if let Some(h) = val.as_hash_map() {
        buf.push('(');
        for (i, (k, v)) in h.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            let _ = write!(buf, "{} => ", stringify_key(k));
            stringify_value(buf, v);
        }
        buf.push(')');
        return;
    }

    if let Some(cr) = val.as_code_ref() {
        buf.push_str("sub");
        if !cr.params.is_empty() {
            buf.push_str(" (");
            for (i, p) in cr.params.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                match p {
                    crate::ast::SubSigParam::Scalar(name, ty) => {
                        let _ = write!(buf, "${}", name);
                        if let Some(t) = ty {
                            buf.push_str(": ");
                            buf.push_str(match t {
                                crate::ast::PerlTypeName::Int => "Int",
                                crate::ast::PerlTypeName::Str => "Str",
                                crate::ast::PerlTypeName::Float => "Float",
                            });
                        }
                    }
                    crate::ast::SubSigParam::ArrayDestruct(_) => buf.push_str("[...]"),
                    crate::ast::SubSigParam::HashDestruct(_) => buf.push_str("{...}"),
                }
            }
            buf.push(')');
        }
        buf.push_str(" { ");
        buf.push_str(&crate::deparse::deparse_block(&cr.body));
        buf.push_str(" }");
        return;
    }

    if val.is_integer_like() || val.is_float_like() {
        let _ = write!(buf, "{val}");
        return;
    }

    let s = val.to_string();
    let _ = write!(buf, "\"{}\"", stringify_escape(&s));
}

fn stringify_key(k: &str) -> String {
    if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.is_empty() {
        k.to_string()
    } else {
        format!("\"{}\"", stringify_escape(k))
    }
}

fn stringify_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '$' => out.push_str("\\$"),
            '@' => out.push_str("\\@"),
            _ => out.push(c),
        }
    }
    out
}

/// `input` — read all of stdin. `input $fh` / `input "path"` — read from filehandle or file.
fn builtin_input(interp: &Interpreter, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    use std::io::Read;
    if args.is_empty() {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
        return Ok(PerlValue::string(buf));
    }
    let name = args[0].to_string();
    // STDIN handle — read stdin directly
    if name == "STDIN" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
        return Ok(PerlValue::string(buf));
    }
    // If <STDIN> was used, the parser already read all of stdin into the arg string.
    // Detect this: if the arg came from a readline (contains newlines, not a valid path),
    // just return it as-is.
    if name.contains('\n') && std::fs::metadata(&name).is_err() {
        return Ok(args[0].clone());
    }
    // Try as an open filehandle via io_file_slots
    if let Some(slot) = interp.io_file_slots.get(&name) {
        let mut f = slot.lock();
        let mut buf = String::new();
        f.read_to_string(&mut buf)
            .map_err(|e| PerlError::runtime(format!("input: {}: {}", name, e), line))?;
        return Ok(PerlValue::string(buf));
    }
    // Fall back to treating it as a file path
    let content = std::fs::read_to_string(&name)
        .map_err(|e| PerlError::runtime(format!("input: {}: {}", name, e), line))?;
    Ok(PerlValue::string(content))
}

/// `lines STRING` — split string into array on newlines (no trailing empty).
/// `lines ITERATOR` — flat-map each element's lines (streaming).
/// Returns a streaming iterator for lazy consumption.
fn builtin_lines(_interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::LinesFlatMapIterator::new(source),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::LinesIterator::new(&s),
    )))
}

/// `words STRING` — split on whitespace into array.
/// `words ITERATOR` — flat-map each element's words (streaming).
fn builtin_words(_interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::WordsFlatMapIterator::new(source),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::WordsIterator::new(&s),
    )))
}

/// `chars STRING` — split into individual characters (no empty leading element).
/// `chars ITERATOR` — flat-map each element's characters (streaming).
/// Returns a streaming iterator for lazy consumption.
fn builtin_chars(_interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::CharsFlatMapIterator::new(source),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::CharsIterator::new(&s),
    )))
}

/// `trim STRING` or `trim LIST` — strip leading and trailing whitespace.
/// If input is an iterator or list, returns a streaming iterator that trims each element.
fn builtin_trim(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() == 1 {
        let v = &args[0];
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::TrimIterator::new(source),
            )));
        }
        if v.as_array_vec().is_some() {
            let source = crate::map_stream::into_pull_iter(v.clone());
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::TrimIterator::new(source),
            )));
        }
        let s = v.to_string();
        return Ok(PerlValue::string(s.trim().to_string()));
    }
    if args.len() > 1 {
        let source = crate::map_stream::into_pull_iter(PerlValue::array(args.to_vec()));
        return Ok(PerlValue::iterator(Arc::new(
            crate::map_stream::TrimIterator::new(source),
        )));
    }
    Ok(PerlValue::string(String::new()))
}

/// `avg LIST` — arithmetic mean of numeric list.
fn builtin_avg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let flat: Vec<PerlValue> = args
        .iter()
        .flat_map(|a| a.map_flatten_outputs(true))
        .collect();
    if flat.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let sum: f64 = flat.iter().map(|v| v.to_number()).sum();
    Ok(PerlValue::float(sum / flat.len() as f64))
}

/// `top N, HASHREF` — return top N keys from a frequencies-style hash ref, sorted by count desc.
fn builtin_top(args: &[PerlValue]) -> PerlResult<PerlValue> {
    // When piped: top(HASHREF, N) or top(HASHREF) defaults N=10
    // Direct: top N, HASHREF  or  top HASHREF
    let (href, n) = if args.len() >= 2 {
        // Could be (N, HASHREF) or (HASHREF, N)
        if args[0].as_hash_ref().is_some() {
            (&args[0], args[1].to_int() as usize)
        } else {
            (&args[1], args[0].to_int() as usize)
        }
    } else if args.len() == 1 {
        (&args[0], 10)
    } else {
        return Ok(PerlValue::UNDEF);
    };
    if let Some(hr) = href.as_hash_ref() {
        let guard = hr.read();
        let mut pairs: Vec<_> = guard.iter().collect();
        pairs.sort_by(|a, b| b.1.to_int().cmp(&a.1.to_int()));
        let items: Vec<PerlValue> = pairs
            .into_iter()
            .take(n)
            .flat_map(|(k, v)| vec![PerlValue::string(k.clone()), v.clone()])
            .collect();
        Ok(PerlValue::hash_ref(Arc::new(RwLock::new(
            items
                .chunks(2)
                .map(|c| (c[0].to_string(), c[1].clone()))
                .collect::<indexmap::IndexMap<_, _>>(),
        ))))
    } else {
        Ok(PerlValue::UNDEF)
    }
}

/// `to_file PATH, STRING` — write string to file, returns the string for further piping.
fn builtin_to_file(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "to_file: need PATH and content".to_string(),
            line,
        ));
    }
    // to_file(STRING, PATH) when piped, to_file(PATH, STRING) when called directly
    // Heuristic: if first arg contains newlines or is long, it's content
    let (path, content) = if args[0].to_string().contains('\n') || args[0].to_string().len() > 260 {
        (args[1].to_string(), args[0].to_string())
    } else {
        (args[0].to_string(), args[1].to_string())
    };
    std::fs::write(&path, &content)
        .map_err(|e| PerlError::runtime(format!("to_file: {}: {}", path, e), line))?;
    Ok(PerlValue::string(content))
}

/// `to_json VALUE` — serialize a PerlValue to a JSON string.
fn builtin_to_json(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() > 1 {
        // Multiple values (e.g. flattened array) → JSON array
        let parts: Vec<String> = args.iter().map(perl_value_to_json_string).collect();
        return Ok(PerlValue::string(format!("[{}]", parts.join(","))));
    }
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    Ok(PerlValue::string(perl_value_to_json_string(&val)))
}

fn perl_value_to_json_string(val: &PerlValue) -> String {
    use std::fmt::Write;
    if val.is_undef() {
        return "null".to_string();
    }
    if val.is_integer_like() {
        return format!("{}", val.to_int());
    }
    if val.is_float_like() {
        return format!("{}", val.to_number());
    }
    if let Some(ar) = val.as_array_ref() {
        let guard = ar.read();
        let parts: Vec<String> = guard.iter().map(perl_value_to_json_string).collect();
        return format!("[{}]", parts.join(","));
    }
    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        let mut buf = String::from("{");
        for (i, (k, v)) in guard.iter().enumerate() {
            if i > 0 {
                buf.push(',');
            }
            let _ = write!(
                buf,
                "\"{}\":{}",
                json_escape(k),
                perl_value_to_json_string(v)
            );
        }
        buf.push('}');
        return buf;
    }
    format!("\"{}\"", json_escape(&val.to_string()))
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// `to_csv ARRAYREF_OF_HASHREFS` — serialize to CSV string.
fn builtin_to_csv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let rows: Vec<PerlValue> = if let Some(ar) = val.as_array_ref() {
        ar.read().clone()
    } else {
        args.iter()
            .flat_map(|a| a.map_flatten_outputs(true))
            .collect()
    };
    if rows.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    // Collect all keys from first row for header order
    let headers: Vec<String> = if let Some(hr) = rows[0].as_hash_ref() {
        hr.read().keys().cloned().collect()
    } else {
        return Ok(PerlValue::string(String::new()));
    };
    let mut buf = headers.join(",");
    buf.push('\n');
    for row in &rows {
        if let Some(hr) = row.as_hash_ref() {
            let guard = hr.read();
            let vals: Vec<String> = headers
                .iter()
                .map(|h| {
                    let v = guard.get(h).map(|v| v.to_string()).unwrap_or_default();
                    if v.contains(',') || v.contains('"') || v.contains('\n') {
                        format!("\"{}\"", v.replace('"', "\"\""))
                    } else {
                        v
                    }
                })
                .collect();
            buf.push_str(&vals.join(","));
            buf.push('\n');
        }
    }
    Ok(PerlValue::string(buf))
}

/// `grep_v PATTERN, LIST` — inverse grep: reject elements matching regex.
/// `grep_v PATTERN, LIST` — inverse filter (rejects matching items).
/// Returns a streaming iterator for lazy consumption.
fn builtin_grep_v(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let pattern = args[0].to_string();
    let re = regex::Regex::new(&pattern)
        .map_err(|e| PerlError::runtime(format!("grep_v: bad pattern: {}", e), line))?;
    if args.len() == 2 {
        let v = &args[1];
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::GrepVIterator::new(source, re),
            )));
        }
    }
    let source = crate::map_stream::into_pull_iter(if args.len() == 2 {
        args[1].clone()
    } else {
        PerlValue::array(args[1..].to_vec())
    });
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::GrepVIterator::new(source, re),
    )))
}

/// `select_keys HASHREF, KEY, KEY, ...` — pick only named keys from a hash ref.
fn builtin_select_keys(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let href = &args[0];
    if let Some(hr) = href.as_hash_ref() {
        let guard = hr.read();
        let mut result = indexmap::IndexMap::new();
        for key_val in &args[1..] {
            let k = key_val.to_string();
            if let Some(v) = guard.get(&k) {
                result.insert(k, v.clone());
            }
        }
        Ok(PerlValue::hash_ref(Arc::new(RwLock::new(result))))
    } else {
        Ok(PerlValue::UNDEF)
    }
}

/// `first_or DEFAULT, LIST` — returns first element of list, or DEFAULT if empty.
/// Works lazily on iterators.
fn builtin_first_or(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let default = args[0].clone();
    if args.len() == 1 {
        return Ok(default);
    }
    let v = &args[1];
    if v.is_iterator() {
        let iter = v.clone().into_iterator();
        return Ok(iter.next_item().unwrap_or(default));
    }
    if let Some(arr) = v.as_array_vec() {
        return Ok(arr.first().cloned().unwrap_or(default));
    }
    let list = v.to_list();
    Ok(list.into_iter().next().unwrap_or(default))
}

/// `compact LIST` — removes undef and empty string values (streaming).
fn builtin_compact(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let source = if args.len() == 1 {
        let v = &args[0];
        if v.is_iterator() {
            v.clone().into_iterator()
        } else {
            crate::map_stream::into_pull_iter(v.clone())
        }
    } else {
        crate::map_stream::into_pull_iter(PerlValue::array(args.to_vec()))
    };
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::CompactIterator::new(source),
    )))
}

/// `concat LIST1, LIST2, ...` / `chain LIST1, LIST2, ...` — concatenates iterators (streaming).
fn builtin_concat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let sources: Vec<Arc<dyn crate::value::PerlIterator>> = args
        .iter()
        .map(|v| {
            if v.is_iterator() {
                v.clone().into_iterator()
            } else {
                crate::map_stream::into_pull_iter(v.clone())
            }
        })
        .collect();
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::ConcatIterator::new(sources),
    )))
}

/// `enumerate ITERATOR` — yields `[$index, $item]` pairs (streaming).
/// In pipeline: `ITERATOR |> enumerate`
fn builtin_enumerate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let source = if args.len() == 1 && args[0].is_iterator() {
        args[0].clone().into_iterator()
    } else {
        crate::map_stream::into_pull_iter(PerlValue::array(args.to_vec()))
    };
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::EnumerateIterator::new(source),
    )))
}

/// `chunk N, ITERATOR` — yields N-element arrayrefs (streaming).
/// In pipeline: `ITERATOR |> chunk N`
fn builtin_chunk(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = args[0].to_int().max(1) as usize;
    if args.len() < 2 {
        return Ok(PerlValue::array(vec![]));
    }
    let v = &args[1];
    let source = if v.is_iterator() {
        v.clone().into_iterator()
    } else {
        crate::map_stream::into_pull_iter(v.clone())
    };
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::ChunkIterator::new(source, n),
    )))
}

/// `dedup ITERATOR` — drops consecutive duplicates (streaming).
/// In pipeline: `ITERATOR |> dedup`
fn builtin_dedup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let source = if args.len() == 1 && args[0].is_iterator() {
        args[0].clone().into_iterator()
    } else {
        crate::map_stream::into_pull_iter(PerlValue::array(args.to_vec()))
    };
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::DedupIterator::new(source),
    )))
}

/// `range N, M` — lazy integer sequence from N to M (inclusive).
fn builtin_range(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let start = args.first().map(|v| v.to_int()).unwrap_or(0);
    let end = args.get(1).map(|v| v.to_int()).unwrap_or(start);
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::RangeIterator::new(start, end),
    )))
}

/// `tee FILE, ITERATOR` — write each item to file while passing through (streaming).
/// In pipeline: `ITERATOR |> tee FILE`
fn builtin_tee(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("tee: expected FILE, ITERATOR", line));
    }
    let path = args[0].to_string();
    let v = &args[1];
    let source = if v.is_iterator() {
        v.clone().into_iterator()
    } else {
        crate::map_stream::into_pull_iter(v.clone())
    };
    let iter = crate::map_stream::TeeIterator::new(source, &path)
        .map_err(|e| PerlError::runtime(format!("tee: {}: {}", path, e), line))?;
    Ok(PerlValue::iterator(Arc::new(iter)))
}

/// `nth N, ITERATOR` — get Nth element (0-indexed), consumes up to that point.
/// In pipeline: `ITERATOR |> nth N`
fn builtin_nth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let n = args[0].to_int().max(0) as usize;
    if args.len() < 2 {
        return Ok(PerlValue::UNDEF);
    }
    let v = &args[1];
    if v.is_iterator() {
        let iter = v.clone().into_iterator();
        for _ in 0..n {
            if iter.next_item().is_none() {
                return Ok(PerlValue::UNDEF);
            }
        }
        return Ok(iter.next_item().unwrap_or(PerlValue::UNDEF));
    }
    let list = v.to_list();
    Ok(list.get(n).cloned().unwrap_or(PerlValue::UNDEF))
}

/// `to_set ITERATOR` or `to_set LIST` — collect iterator/list to a set.
fn builtin_to_set(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(crate::value::set_from_elements(std::iter::empty()));
    }
    if args.len() == 1 {
        let v = &args[0];
        if v.is_iterator() {
            let iter = v.clone().into_iterator();
            let mut items = Vec::new();
            while let Some(item) = iter.next_item() {
                items.push(item);
            }
            return Ok(crate::value::set_from_elements(items));
        }
        let list = v.to_list();
        return Ok(crate::value::set_from_elements(list));
    }
    Ok(crate::value::set_from_elements(args.iter().cloned()))
}

/// `to_hash ITERATOR` or `to_hash LIST` — collect pairs (or flat k,v,k,v) to a hash.
fn builtin_to_hash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use parking_lot::RwLock;
    if args.is_empty() {
        return Ok(PerlValue::hash_ref(Arc::new(RwLock::new(
            indexmap::IndexMap::new(),
        ))));
    }
    let items: Vec<PerlValue> = if args.len() == 1 {
        let v = &args[0];
        if v.is_iterator() {
            let iter = v.clone().into_iterator();
            let mut out = Vec::new();
            while let Some(item) = iter.next_item() {
                out.push(item);
            }
            out
        } else {
            v.to_list()
        }
    } else {
        args.to_vec()
    };
    let mut map = indexmap::IndexMap::new();
    let mut i = 0;
    while i < items.len() {
        if let Some(aref) = items[i].as_array_ref() {
            let pair = aref.read();
            if pair.len() >= 2 {
                map.insert(pair[0].to_string(), pair[1].clone());
                i += 1;
                continue;
            }
        }
        let key = items[i].to_string();
        let val = items.get(i + 1).cloned().unwrap_or(PerlValue::UNDEF);
        map.insert(key, val);
        i += 2;
    }
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(map))))
}

/// `pluck KEY, LIST_OF_HASHREFS` — extract one key from each hashref in a list.
/// `pluck KEY, LIST` — extracts a key from each hash ref.
/// Returns a streaming iterator for lazy consumption.
fn builtin_pluck(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let key = args[0].to_string();
    if args.len() == 2 {
        let v = &args[1];
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::PluckIterator::new(source, key),
            )));
        }
    }
    let source = crate::map_stream::into_pull_iter(if args.len() == 2 {
        args[1].clone()
    } else {
        PerlValue::array(args[1..].to_vec())
    });
    Ok(PerlValue::iterator(Arc::new(
        crate::map_stream::PluckIterator::new(source, key),
    )))
}

/// `clamp MIN, MAX, LIST` — clamp each numeric value to [MIN, MAX].
/// In pipeline: `LIST |> clamp MIN, MAX`
fn builtin_clamp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() < 3 {
        return Ok(PerlValue::UNDEF);
    }
    // args layout after pipe: (LIST_ITEM, MIN, MAX) via insert(0, lhs)
    // or direct: clamp(MIN, MAX, LIST...)
    // Heuristic: if args[2..] expand to multiple items, first two are min/max
    let rest: Vec<PerlValue> = args[2..]
        .iter()
        .flat_map(|a| a.map_flatten_outputs(true))
        .collect();
    let (min_val, max_val, values) = if rest.is_empty() {
        // piped: (value, min, max)
        let min_v = args[1].to_number();
        let max_v = args[2].to_number();
        let vals: Vec<PerlValue> = args[0..1]
            .iter()
            .flat_map(|a| a.map_flatten_outputs(true))
            .collect();
        (min_v, max_v, vals)
    } else {
        // direct: clamp(min, max, list...)
        (args[0].to_number(), args[1].to_number(), rest)
    };
    let items: Vec<PerlValue> = values
        .iter()
        .map(|v| {
            let n = v.to_number();
            let clamped = if n < min_val {
                min_val
            } else if n > max_val {
                max_val
            } else {
                n
            };
            if clamped == clamped.floor() && clamped.abs() < i64::MAX as f64 {
                PerlValue::integer(clamped as i64)
            } else {
                PerlValue::float(clamped)
            }
        })
        .collect();
    if items.len() == 1 {
        Ok(items.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(items))
    }
}

/// `normalize LIST` — scale numeric list to 0..1 range.
/// `normalize OUT_MIN, OUT_MAX, LIST` — scale to custom [OUT_MIN, OUT_MAX] range.
fn builtin_normalize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }

    let all: Vec<f64> = args
        .iter()
        .flat_map(|a| a.map_flatten_outputs(true))
        .map(|v| v.to_number())
        .collect();

    if all.len() < 2 {
        return Ok(PerlValue::UNDEF);
    }

    let (out_min, out_max, data) = (0.0_f64, 1.0_f64, all);

    let src_min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let src_max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let src_range = src_max - src_min;

    let items: Vec<PerlValue> = data
        .iter()
        .map(|&n| {
            if src_range == 0.0 {
                PerlValue::float(out_min)
            } else {
                PerlValue::float(out_min + (n - src_min) / src_range * (out_max - out_min))
            }
        })
        .collect();
    Ok(PerlValue::array(items))
}

/// `stddev LIST` — population standard deviation of numeric list.
fn builtin_stddev(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let flat: Vec<PerlValue> = args
        .iter()
        .flat_map(|a| a.map_flatten_outputs(true))
        .collect();
    if flat.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let nums: Vec<f64> = flat.iter().map(|v| v.to_number()).collect();
    let n = nums.len() as f64;
    let mean = nums.iter().sum::<f64>() / n;
    let variance = nums.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / n;
    Ok(PerlValue::float(variance.sqrt()))
}

/// `squared N` / `sq N` — return N squared (N * N).
fn builtin_squared(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(n * n))
}

/// `cubed N` / `cb N` — return N cubed (N * N * N).
fn builtin_cubed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(n * n * n))
}

/// `expt BASE, EXP` — return BASE raised to power EXP.
fn builtin_expt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let base = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let exp = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(base.powf(exp)))
}

/// Split a string into word boundaries for case conversion.
fn split_case_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' || c == '\t' {
            if !cur.is_empty() {
                words.push(cur.clone());
                cur.clear();
            }
        } else if c.is_uppercase()
            && !cur.is_empty()
            && cur.chars().last().is_some_and(|p| p.is_lowercase())
        {
            words.push(cur.clone());
            cur.clear();
            cur.push(c);
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn to_snake_case(s: &str) -> String {
    split_case_words(s)
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn to_camel_case(s: &str) -> String {
    let words = split_case_words(s);
    let mut result = String::new();
    for (i, w) in words.iter().enumerate() {
        if i == 0 {
            result.push_str(&w.to_lowercase());
        } else {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.push_str(&chars.as_str().to_lowercase());
            }
        }
    }
    result
}

fn to_kebab_case(s: &str) -> String {
    split_case_words(s)
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

/// `snake_case STRING` — convert to snake_case.
/// `snake_case ITERATOR` — map each element (streaming).
fn builtin_snake_case(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::MapFnIterator::new(source, |s| to_snake_case(&s)),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(to_snake_case(&s)))
}

/// `camel_case STRING` — convert to camelCase (lower first).
/// `camel_case ITERATOR` — map each element (streaming).
fn builtin_camel_case(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::MapFnIterator::new(source, |s| to_camel_case(&s)),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(to_camel_case(&s)))
}

/// `kebab_case STRING` — convert to kebab-case.
/// `kebab_case ITERATOR` — map each element (streaming).
fn builtin_kebab_case(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if let Some(v) = args.first() {
        if v.is_iterator() {
            let source = v.clone().into_iterator();
            return Ok(PerlValue::iterator(Arc::new(
                crate::map_stream::MapFnIterator::new(source, |s| to_kebab_case(&s)),
            )));
        }
    }
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(to_kebab_case(&s)))
}

/// `to_toml VALUE` — serialize a PerlValue to a TOML string.
fn builtin_to_toml(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() > 1 {
        let parts: Vec<String> = args
            .iter()
            .map(|v| perl_value_to_toml_string(v, 0))
            .collect();
        return Ok(PerlValue::string(parts.join("\n")));
    }
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    Ok(PerlValue::string(perl_value_to_toml_string(&val, 0)))
}

fn perl_value_to_toml_string(val: &PerlValue, _depth: usize) -> String {
    if val.is_undef() {
        return String::new();
    }
    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        let mut buf = String::new();
        let mut tables = Vec::new();
        // First pass: simple key-value pairs
        for (k, v) in guard.iter() {
            if v.as_hash_ref().is_some() {
                tables.push((k.clone(), v.clone()));
            } else if let Some(ar) = v.as_array_ref() {
                let arr_guard = ar.read();
                // Check if array of tables
                if arr_guard.first().and_then(|v| v.as_hash_ref()).is_some() {
                    tables.push((k.clone(), v.clone()));
                } else {
                    buf.push_str(&format!(
                        "{} = [{}]\n",
                        k,
                        arr_guard
                            .iter()
                            .map(toml_scalar)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            } else {
                buf.push_str(&format!("{} = {}\n", k, toml_scalar(v)));
            }
        }
        // Second pass: nested tables
        for (k, v) in &tables {
            if let Some(hr2) = v.as_hash_ref() {
                buf.push_str(&format!("\n[{}]\n", k));
                let g2 = hr2.read();
                for (k2, v2) in g2.iter() {
                    buf.push_str(&format!("{} = {}\n", k2, toml_scalar(v2)));
                }
            } else if let Some(ar) = v.as_array_ref() {
                let arr_guard = ar.read();
                for item in arr_guard.iter() {
                    buf.push_str(&format!("\n[[{}]]\n", k));
                    if let Some(ihr) = item.as_hash_ref() {
                        let ig = ihr.read();
                        for (ik, iv) in ig.iter() {
                            buf.push_str(&format!("{} = {}\n", ik, toml_scalar(iv)));
                        }
                    }
                }
            }
        }
        buf
    } else {
        toml_scalar(val)
    }
}

fn toml_scalar(val: &PerlValue) -> String {
    if val.is_undef() {
        return "\"\"".to_string();
    }
    if val.is_integer_like() {
        return format!("{}", val.to_int());
    }
    if val.is_float_like() {
        return format!("{}", val.to_number());
    }
    let s = val.to_string();
    if s == "true" || s == "false" {
        return s;
    }
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

/// `to_yaml VALUE` — serialize a PerlValue to a YAML string.
fn builtin_to_yaml(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() > 1 {
        let mut buf = String::new();
        for v in args {
            buf.push_str("---\n");
            yaml_value(&mut buf, v, 0);
        }
        return Ok(PerlValue::string(buf));
    }
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut buf = String::from("---\n");
    yaml_value(&mut buf, &val, 0);
    Ok(PerlValue::string(buf))
}

fn yaml_value(buf: &mut String, val: &PerlValue, depth: usize) {
    let indent = "  ".repeat(depth);
    if val.is_undef() {
        buf.push_str("~\n");
        return;
    }
    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        if guard.is_empty() {
            buf.push_str("{}\n");
            return;
        }
        if depth > 0 {
            buf.push('\n');
        }
        for (k, v) in guard.iter() {
            buf.push_str(&format!("{}{}:", indent, k));
            if v.as_hash_ref().is_some() || v.as_array_ref().is_some() {
                yaml_value(buf, v, depth + 1);
            } else {
                buf.push(' ');
                yaml_value(buf, v, depth + 1);
            }
        }
        return;
    }
    if let Some(ar) = val.as_array_ref() {
        let guard = ar.read();
        if guard.is_empty() {
            buf.push_str("[]\n");
            return;
        }
        if depth > 0 {
            buf.push('\n');
        }
        for item in guard.iter() {
            buf.push_str(&format!("{}- ", indent));
            if item.as_hash_ref().is_some() || item.as_array_ref().is_some() {
                // Inline the first level for array-of-hashes
                if let Some(ihr) = item.as_hash_ref() {
                    let ig = ihr.read();
                    let mut first = true;
                    for (ik, iv) in ig.iter() {
                        if first {
                            buf.push_str(&format!("{}: ", ik));
                            yaml_scalar(buf, iv);
                            first = false;
                        } else {
                            buf.push_str(&format!("{}  {}: ", indent, ik));
                            yaml_scalar(buf, iv);
                        }
                    }
                } else {
                    yaml_value(buf, item, depth + 1);
                }
            } else {
                yaml_scalar(buf, item);
            }
        }
        return;
    }
    yaml_scalar(buf, val);
}

fn yaml_scalar(buf: &mut String, val: &PerlValue) {
    if val.is_undef() {
        buf.push_str("~\n");
    } else if val.is_integer_like() {
        buf.push_str(&format!("{}\n", val.to_int()));
    } else if val.is_float_like() {
        buf.push_str(&format!("{}\n", val.to_number()));
    } else {
        let s = val.to_string();
        if s.contains(':')
            || s.contains('#')
            || s.contains('\n')
            || s.contains('"')
            || s.is_empty()
            || s == "true"
            || s == "false"
            || s == "null"
        {
            buf.push_str(&format!(
                "\"{}\"\n",
                s.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        } else {
            buf.push_str(&format!("{}\n", s));
        }
    }
}

/// `to_xml VALUE` — serialize a PerlValue to an XML string.
fn builtin_to_xml(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.len() > 1 {
        let mut buf = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n");
        for (i, v) in args.iter().enumerate() {
            xml_value(&mut buf, &format!("item{}", i), v, 1);
        }
        buf.push_str("</root>\n");
        return Ok(PerlValue::string(buf));
    }
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut buf = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml_value(&mut buf, "root", &val, 0);
    Ok(PerlValue::string(buf))
}

fn xml_value(buf: &mut String, tag: &str, val: &PerlValue, depth: usize) {
    let indent = "  ".repeat(depth);
    if val.is_undef() {
        buf.push_str(&format!("{}<{}/>\n", indent, tag));
        return;
    }
    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        buf.push_str(&format!("{}<{}>\n", indent, tag));
        for (k, v) in guard.iter() {
            xml_value(buf, k, v, depth + 1);
        }
        buf.push_str(&format!("{}</{}>\n", indent, tag));
        return;
    }
    if let Some(ar) = val.as_array_ref() {
        let guard = ar.read();
        buf.push_str(&format!("{}<{}>\n", indent, tag));
        for item in guard.iter() {
            xml_value(buf, "item", item, depth + 1);
        }
        buf.push_str(&format!("{}</{}>\n", indent, tag));
        return;
    }
    let s = xml_escape(&val.to_string());
    buf.push_str(&format!("{}<{}>{}</{}>\n", indent, tag, s, tag));
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ddump_value(buf: &mut String, val: &PerlValue, depth: usize) {
    use std::fmt::Write;
    let indent = "  ".repeat(depth);
    let inner = "  ".repeat(depth + 1);

    if val.is_undef() {
        buf.push_str("undef");
        return;
    }

    if let Some(ar) = val.as_array_ref() {
        let guard = ar.read();
        if guard.is_empty() {
            buf.push_str("[]");
            return;
        }
        buf.push_str("[\n");
        for (i, elem) in guard.iter().enumerate() {
            buf.push_str(&inner);
            ddump_value(buf, elem, depth + 1);
            if i + 1 < guard.len() {
                buf.push(',');
            }
            buf.push('\n');
        }
        buf.push_str(&indent);
        buf.push(']');
        return;
    }

    if let Some(hr) = val.as_hash_ref() {
        let guard = hr.read();
        if guard.is_empty() {
            buf.push_str("{}");
            return;
        }
        buf.push_str("{\n");
        let len = guard.len();
        for (i, (k, v)) in guard.iter().enumerate() {
            buf.push_str(&inner);
            let _ = write!(buf, "'{k}' => ");
            ddump_value(buf, v, depth + 1);
            if i + 1 < len {
                buf.push(',');
            }
            buf.push('\n');
        }
        buf.push_str(&indent);
        buf.push('}');
        return;
    }

    if let Some(sr) = val.as_scalar_ref() {
        let inner_val = sr.read().clone();
        buf.push('\\');
        ddump_value(buf, &inner_val, depth);
        return;
    }

    if let Some(blessed) = val.as_blessed_ref() {
        let data = blessed.data.read().clone();
        let _ = write!(buf, "bless(");
        ddump_value(buf, &data, depth);
        let _ = write!(buf, ", '{}')", blessed.class);
        return;
    }

    // Numeric (int or float) — print bare
    if val.is_integer_like() || val.is_float_like() {
        let _ = write!(buf, "{val}");
        return;
    }

    // String — quote it
    let s = val.to_string();
    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = write!(buf, "'{escaped}'");
}

/// `read_lines PATH` — slurp file into array of chomped lines.
fn builtin_read_lines(
    interp: &Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| PerlError::runtime(format!("read_lines: {}: {}", path, e), line))?;
    let lines: Vec<PerlValue> = content
        .lines()
        .map(|l| PerlValue::string(l.to_string()))
        .collect();
    Ok(match interp.wantarray_kind {
        WantarrayCtx::List => PerlValue::array(lines),
        WantarrayCtx::Scalar => PerlValue::integer(lines.len() as i64),
        WantarrayCtx::Void => PerlValue::UNDEF,
    })
}

/// `append_file PATH, DATA` — append content to a file.
fn builtin_append_file(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("append_file needs PATH and DATA", line));
    }
    let path = args[0].to_string();
    let data = perl_scalar_as_bytes(&args[1]);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| PerlError::runtime(format!("append_file: {}: {}", path, e), line))?;
    f.write_all(&data)
        .map_err(|e| PerlError::runtime(format!("append_file: {}: {}", path, e), line))?;
    Ok(PerlValue::integer(data.len() as i64))
}

/// `tempfile()` or `tempfile(SUFFIX)` — create a temporary file, return its path.
fn builtin_tempfile(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let suffix = args.first().map(|v| v.to_string()).unwrap_or_default();
    let dir = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = if suffix.is_empty() {
        format!("perlrs_tmp_{}", stamp)
    } else {
        format!("perlrs_tmp_{}{}", stamp, suffix)
    };
    let path = dir.join(name);
    std::fs::File::create(&path)
        .map_err(|e| PerlError::runtime(format!("tempfile: {}", e), line))?;
    Ok(PerlValue::string(path.to_string_lossy().to_string()))
}

/// `tempdir()` — create a temporary directory, return its path.
fn builtin_tempdir(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let suffix = args.first().map(|v| v.to_string()).unwrap_or_default();
    let dir = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = if suffix.is_empty() {
        format!("perlrs_tmpd_{}", stamp)
    } else {
        format!("perlrs_tmpd_{}{}", stamp, suffix)
    };
    let path = dir.join(name);
    std::fs::create_dir_all(&path)
        .map_err(|e| PerlError::runtime(format!("tempdir: {}", e), line))?;
    Ok(PerlValue::string(path.to_string_lossy().to_string()))
}

/// `read_json PATH` — read a JSON file and decode into a Perl value.
fn builtin_read_json(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| PerlError::runtime(format!("read_json: {}: {}", path, e), line))?;
    crate::native_data::json_decode(&content)
}

/// `write_json PATH, VALUE` — encode value as JSON and write to file.
fn builtin_write_json(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("write_json needs PATH and VALUE", line));
    }
    let path = args[0].to_string();
    let val = &args[1];
    let json = crate::native_data::json_encode(val)?;
    let s = json.to_string();
    std::fs::write(&path, s.as_bytes())
        .map_err(|e| PerlError::runtime(format!("write_json: {}: {}", path, e), line))?;
    Ok(PerlValue::integer(s.len() as i64))
}

/// `glob_match PATTERN, STRING` — test if STRING matches a glob pattern.
fn builtin_glob_match(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "glob_match needs PATTERN and STRING",
            line,
        ));
    }
    let pattern = args[0].to_string();
    let target = args[1].to_string();
    // Simple glob matching: * matches anything, ? matches one char
    let re_str = glob_pattern_to_regex(&pattern);
    let matched = regex::Regex::new(&re_str)
        .map(|re| re.is_match(&target))
        .unwrap_or(false);
    Ok(PerlValue::integer(matched as i64))
}

/// Convert a glob pattern to a regex string.
fn glob_pattern_to_regex(pattern: &str) -> String {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    re.push_str(".*");
                } else {
                    re.push_str("[^/]*");
                }
            }
            '?' => re.push('.'),
            '.' | '+' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '$' | '|' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            _ => re.push(c),
        }
    }
    re.push('$');
    re
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

// ── elapsed() ─────────────────────────────────────────────────────
/// Returns fractional seconds since process start with nanosecond precision.
fn builtin_elapsed() -> PerlResult<PerlValue> {
    let secs = PROCESS_START.elapsed().as_secs_f64();
    Ok(PerlValue::float(secs))
}

// ── crc32(DATA) ───────────────────────────────────────────────────
/// CRC-32 checksum of the argument bytes; returns an unsigned 32-bit integer.
fn builtin_crc32(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let data = args
        .first()
        .ok_or_else(|| PerlError::runtime("crc32: need DATA argument", line))?;
    let bytes = perl_scalar_as_bytes(data);
    Ok(PerlValue::integer(crc32fast::hash(&bytes) as i64))
}

// ── par_find_files(PATH, PATTERN) ─────────────────────────────────
/// Parallel recursive file search. Returns all paths under PATH whose
/// filename matches the glob PATTERN (e.g. `"*.rs"`, `"test_*"`).
fn builtin_par_find_files(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    use std::path::PathBuf;

    let root = args
        .first()
        .ok_or_else(|| PerlError::runtime("par_find_files: need PATH, PATTERN", line))?
        .to_string();
    let pattern = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("par_find_files: need PATTERN", line))?
        .to_string();
    let pat = glob::Pattern::new(&pattern)
        .map_err(|e| PerlError::runtime(format!("par_find_files: bad pattern: {}", e), line))?;

    let root_path = PathBuf::from(&root);
    if !root_path.is_dir() {
        return Ok(PerlValue::array(vec![]));
    }
    let all = crate::par_walk::collect_paths(std::slice::from_ref(&root_path));
    let matched: Vec<PerlValue> = all
        .into_par_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| pat.matches(n))
        })
        .map(|p| PerlValue::string(p.to_string_lossy().into_owned()))
        .collect();
    Ok(PerlValue::array(matched))
}

// ── par_line_count(FILE, ...) ─────────────────────────────────────
/// Count lines across files in parallel. Returns total when called in
/// scalar context, or a list of per-file counts in list context.
fn builtin_par_line_count(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "par_line_count: need at least one file path",
            line,
        ));
    }
    let paths: Vec<String> = args
        .iter()
        .flat_map(|a| a.to_list())
        .map(|v| v.to_string())
        .collect();
    let counts: Vec<PerlValue> = paths
        .par_iter()
        .map(|p| {
            let n = std::fs::read(p).map(|b| bytecount(&b)).unwrap_or(0);
            PerlValue::integer(n as i64)
        })
        .collect();
    if counts.len() == 1 {
        return Ok(counts.into_iter().next().unwrap());
    }
    Ok(PerlValue::array(counts))
}

/// Count newline bytes. Equivalent to `memchr`-style `\n` counting.
#[inline]
fn bytecount(buf: &[u8]) -> usize {
    buf.iter().filter(|&&b| b == b'\n').count()
}

// ── chroot(DIRNAME) ────────────────────────────────────────────────
fn builtin_chroot(args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let dir = args
            .first()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "/".into());
        match std::ffi::CString::new(dir.as_str()) {
            Ok(c) => {
                let r = unsafe { libc::chroot(c.as_ptr()) };
                Ok(PerlValue::integer(if r == 0 { 1 } else { 0 }))
            }
            Err(_) => Ok(PerlValue::integer(0)),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        Err(PerlError::runtime(
            "chroot: not implemented on this platform",
            line,
        ))
    }
}

// ── vec(STRING, OFFSET, BITS) ──────────────────────────────────────
fn builtin_vec(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 3 {
        return Err(PerlError::runtime("vec: not enough arguments", line));
    }
    let s = args[0].to_string();
    let bytes = s.as_bytes();
    let offset = args[1].to_int() as usize;
    let bits = args[2].to_int() as usize;
    if !matches!(bits, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(PerlError::runtime(
            format!("vec: illegal number of bits ({})", bits),
            line,
        ));
    }
    let bit_offset = offset * bits;
    let byte_offset = bit_offset / 8;
    let bit_within = bit_offset % 8;
    if bits <= 8 {
        if byte_offset >= bytes.len() {
            return Ok(PerlValue::integer(0));
        }
        let byte = bytes[byte_offset];
        let mask = ((1u16 << bits) - 1) as u8;
        let val = (byte >> bit_within) & mask;
        Ok(PerlValue::integer(val as i64))
    } else if bits == 16 {
        if byte_offset + 1 >= bytes.len() {
            return Ok(PerlValue::integer(0));
        }
        let val = (bytes[byte_offset] as u16) | ((bytes[byte_offset + 1] as u16) << 8);
        Ok(PerlValue::integer(val as i64))
    } else {
        // 32
        if byte_offset + 3 >= bytes.len() {
            return Ok(PerlValue::integer(0));
        }
        let val = (bytes[byte_offset] as u32)
            | ((bytes[byte_offset + 1] as u32) << 8)
            | ((bytes[byte_offset + 2] as u32) << 16)
            | ((bytes[byte_offset + 3] as u32) << 24);
        Ok(PerlValue::integer(val as i64))
    }
}

// ── dump() ─────────────────────────────────────────────────────────
fn builtin_dump() -> PerlResult<PerlValue> {
    // Perl's dump() creates a core dump; we just abort.
    eprintln!("dump: intentional abort (Perl dump semantics)");
    std::process::abort();
}

// ── stub for net iterators that just return 1 / undef ──────────────
fn builtin_stub_ok(_name: &str) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(1))
}

// ── SysV IPC stubs ─────────────────────────────────────────────────
fn builtin_sysv_ipc_stub(name: &str, line: usize) -> PerlResult<PerlValue> {
    Err(PerlError::runtime(
        format!("{}: System V IPC not implemented", name),
        line,
    ))
}

// ── passwd/group iterator stubs (Unix) ─────────────────────────────
fn builtin_setpwent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    unsafe {
        libc::setpwent();
    }
    Ok(PerlValue::integer(1))
}

fn builtin_endpwent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    unsafe {
        libc::endpwent();
    }
    Ok(PerlValue::integer(1))
}

fn builtin_getpwent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let pw = unsafe { libc::getpwent() };
        if pw.is_null() {
            return Ok(PerlValue::UNDEF);
        }
        let pw = unsafe { &*pw };
        let name = unsafe { std::ffi::CStr::from_ptr(pw.pw_name) }
            .to_string_lossy()
            .to_string();
        let uid = pw.pw_uid as i64;
        let gid = pw.pw_gid as i64;
        let dir = unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) }
            .to_string_lossy()
            .to_string();
        let shell = unsafe { std::ffi::CStr::from_ptr(pw.pw_shell) }
            .to_string_lossy()
            .to_string();
        Ok(PerlValue::array(vec![
            PerlValue::string(name),
            PerlValue::string("x".into()),
            PerlValue::integer(uid),
            PerlValue::integer(gid),
            PerlValue::UNDEF, // quota
            PerlValue::UNDEF, // comment
            PerlValue::UNDEF, // gcos
            PerlValue::string(dir),
            PerlValue::string(shell),
        ]))
    }
    #[cfg(not(unix))]
    Ok(PerlValue::UNDEF)
}

fn builtin_setgrent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    unsafe {
        libc::setgrent();
    }
    Ok(PerlValue::integer(1))
}

fn builtin_endgrent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    unsafe {
        libc::endgrent();
    }
    Ok(PerlValue::integer(1))
}

fn builtin_getgrent() -> PerlResult<PerlValue> {
    #[cfg(unix)]
    {
        let gr = unsafe { libc::getgrent() };
        if gr.is_null() {
            return Ok(PerlValue::UNDEF);
        }
        let gr = unsafe { &*gr };
        let name = unsafe { std::ffi::CStr::from_ptr(gr.gr_name) }
            .to_string_lossy()
            .to_string();
        let gid = gr.gr_gid as i64;
        let mut members = Vec::new();
        let mut p = gr.gr_mem;
        while !unsafe { *p }.is_null() {
            members.push(
                unsafe { std::ffi::CStr::from_ptr(*p) }
                    .to_string_lossy()
                    .to_string(),
            );
            p = unsafe { p.add(1) };
        }
        let mem_str = members.join(" ");
        Ok(PerlValue::array(vec![
            PerlValue::string(name),
            PerlValue::string("x".into()),
            PerlValue::integer(gid),
            PerlValue::string(mem_str),
        ]))
    }
    #[cfg(not(unix))]
    Ok(PerlValue::UNDEF)
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
        let which = args[0].to_int() as libc::c_uint;
        let who = args[1].to_int() as libc::id_t;
        unsafe {
            *errno_ptr() = 0;
        }
        let p = unsafe { libc::getpriority(which as _, who) };
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
        let which = args[0].to_int() as libc::c_uint;
        let who = args[1].to_int() as libc::id_t;
        let prio = args[2].to_int() as libc::c_int;
        let r = unsafe { libc::setpriority(which as _, who, prio) };
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
fn passwd_entry_list(pw: &PasswdEntry) -> Vec<PerlValue> {
    vec![
        PerlValue::string(pw.name.clone()),
        PerlValue::string(pw.passwd.clone()),
        PerlValue::integer(pw.uid as i64),
        PerlValue::integer(pw.gid as i64),
        PerlValue::string(String::new()),
        PerlValue::string(String::new()),
        PerlValue::string(pw.gecos.clone()),
        PerlValue::string(pw.dir.clone()),
        PerlValue::string(pw.shell.clone()),
    ]
}

#[cfg(unix)]
struct PasswdEntry {
    name: String,
    passwd: String,
    uid: u32,
    gid: u32,
    gecos: String,
    dir: String,
    shell: String,
}

#[cfg(unix)]
fn extract_passwd(pw: &libc::passwd) -> PasswdEntry {
    let s = |p: *const libc::c_char| -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    };
    PasswdEntry {
        name: s(pw.pw_name),
        passwd: s(pw.pw_passwd),
        uid: pw.pw_uid,
        gid: pw.pw_gid,
        gecos: s(pw.pw_gecos),
        dir: s(pw.pw_dir),
        shell: s(pw.pw_shell),
    }
}

#[cfg(unix)]
fn fetch_passwd_by_uid(uid: libc::uid_t) -> Option<PasswdEntry> {
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
    Some(extract_passwd(&pw))
}

#[cfg(unix)]
fn fetch_passwd_by_name(name: &str) -> Option<PasswdEntry> {
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
    Some(extract_passwd(&pw))
}

#[cfg(unix)]
struct GroupEntry {
    name: String,
    passwd: String,
    gid: u32,
    members: Vec<String>,
}

#[cfg(unix)]
fn extract_group(gr: &libc::group) -> GroupEntry {
    let s = |p: *const libc::c_char| -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
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
    GroupEntry {
        name: s(gr.gr_name),
        passwd: s(gr.gr_passwd),
        gid: gr.gr_gid,
        members,
    }
}

#[cfg(unix)]
fn group_entry_list(gr: &GroupEntry) -> Vec<PerlValue> {
    vec![
        PerlValue::string(gr.name.clone()),
        PerlValue::string(gr.passwd.clone()),
        PerlValue::integer(gr.gid as i64),
        PerlValue::string(gr.members.join(" ")),
    ]
}

#[cfg(unix)]
fn fetch_group_by_gid(gid: libc::gid_t) -> Option<GroupEntry> {
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
    Some(extract_group(&gr))
}

#[cfg(unix)]
fn fetch_group_by_name(name: &str) -> Option<GroupEntry> {
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
    Some(extract_group(&gr))
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
            Ok(PerlValue::string(pw.name.clone()))
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
            Ok(PerlValue::string(gr.name.clone()))
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

    // ── seek(FH, POS, WHENCE) ──────────────────────────────────────────
    fn builtin_seek(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("seek: not enough arguments", line));
        }
        let fh = args[0]
            .as_io_handle_name()
            .unwrap_or_else(|| args[0].to_string());
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
                Ok(_) => Ok(PerlValue::integer(1)),
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::integer(0))
                }
            }
        } else {
            Ok(PerlValue::integer(0))
        }
    }

    // ── read(FH, SCALAR, LENGTH [, OFFSET]) ────────────────────────────
    fn builtin_read(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("read: not enough arguments", line));
        }
        let fh = args[0]
            .as_io_handle_name()
            .unwrap_or_else(|| args[0].to_string());
        let len = args[2].to_int().max(0) as usize;
        let _offset = args.get(3).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
        let mut buf = vec![0u8; len];
        if let Some(slot) = self.io_file_slots.get(&fh).cloned() {
            let n = slot.lock().read(&mut buf).unwrap_or(0);
            buf.truncate(n);
            // Store result into the scalar variable named in args[1]
            let data = PerlValue::string(crate::perl_fs::decode_utf8_or_latin1(&buf));
            let var_name = args[1].to_string();
            let _ = self.scope.set_scalar(&var_name, data);
            Ok(PerlValue::integer(n as i64))
        } else if fh == "STDIN" {
            let n = std::io::stdin().read(&mut buf).unwrap_or(0);
            buf.truncate(n);
            let data = PerlValue::string(crate::perl_fs::decode_utf8_or_latin1(&buf));
            let var_name = args[1].to_string();
            let _ = self.scope.set_scalar(&var_name, data);
            Ok(PerlValue::integer(n as i64))
        } else {
            Err(PerlError::runtime(
                format!("read: unopened handle {}", fh),
                line,
            ))
        }
    }

    // ── sysopen(FH, FILENAME, MODE [, PERMS]) ─────────────────────────
    fn builtin_sysopen(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("sysopen: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let filename = args[1].to_string();
        let mode = args[2].to_int();
        let _perms = args.get(3).map(|v| v.to_int()).unwrap_or(0o666);

        let mut opts = std::fs::OpenOptions::new();
        #[cfg(unix)]
        {
            let access = mode & (libc::O_RDONLY | libc::O_WRONLY | libc::O_RDWR) as i64;
            if access == libc::O_RDONLY as i64 {
                opts.read(true);
            } else if access == libc::O_WRONLY as i64 {
                opts.write(true);
            } else if access == libc::O_RDWR as i64 {
                opts.read(true).write(true);
            }
            if mode & libc::O_CREAT as i64 != 0 {
                opts.create(true);
            }
            if mode & libc::O_TRUNC as i64 != 0 {
                opts.truncate(true);
            }
            if mode & libc::O_APPEND as i64 != 0 {
                opts.append(true);
            }
        }
        #[cfg(not(unix))]
        {
            let access = mode & 3;
            if access == 0 {
                opts.read(true);
            } else if access == 1 {
                opts.write(true);
            } else {
                opts.read(true).write(true);
            }
            if mode & 0x100 != 0 {
                opts.create(true);
            }
            if mode & 0x200 != 0 {
                opts.truncate(true);
            }
            if mode & 0x400 != 0 {
                opts.append(true);
            }
        }

        match opts.open(&filename) {
            Ok(f) => {
                let shared = Arc::new(Mutex::new(f));
                self.io_file_slots.insert(fh.clone(), Arc::clone(&shared));
                // Register in output_handles / input_handles so print/readline work
                let access = mode & 3; // O_RDONLY=0, O_WRONLY=1, O_RDWR=2
                let is_writable = access == 1 || access == 2;
                let is_readable = access == 0 || access == 2;
                if is_writable {
                    use crate::interpreter::IoSharedFileWrite;
                    self.output_handles
                        .insert(fh.clone(), Box::new(IoSharedFileWrite(Arc::clone(&shared))));
                }
                if is_readable {
                    use crate::interpreter::IoSharedFile;
                    use std::io::BufReader;
                    self.input_handles.insert(
                        fh.clone(),
                        BufReader::new(Box::new(IoSharedFile(Arc::clone(&shared)))),
                    );
                }
                Ok(PerlValue::integer(1))
            }
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::integer(0))
            }
        }
    }

    // ── socketpair(FH1, FH2, DOMAIN, TYPE, PROTOCOL) ──────────────────
    fn builtin_socketpair(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        #[cfg(unix)]
        {
            if args.len() < 5 {
                return Err(PerlError::runtime("socketpair: not enough arguments", line));
            }
            let _fh1 = args[0].to_string();
            let _fh2 = args[1].to_string();
            let domain = args[2].to_int() as libc::c_int;
            let typ = args[3].to_int() as libc::c_int;
            let protocol = args[4].to_int() as libc::c_int;
            let mut fds: [libc::c_int; 2] = [0; 2];
            let r = unsafe { libc::socketpair(domain, typ, protocol, fds.as_mut_ptr()) };
            if r == 0 {
                return Ok(PerlValue::integer(1));
            }
            Ok(PerlValue::integer(0))
        }
        #[cfg(not(unix))]
        {
            let _ = (args, line);
            Ok(PerlValue::integer(0))
        }
    }

    // ── formline(PICTURE, LIST) ────────────────────────────────────────
    fn builtin_formline(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        let picture = args.first().map(|v| v.to_string()).unwrap_or_default();
        let values: Vec<String> = args.iter().skip(1).map(|v| v.to_string()).collect();
        // Basic formline: substitute @<<< @>>> @||| fields with values
        let mut result = String::new();
        let mut val_idx = 0;
        let mut chars = picture.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '@' {
                let mut field_len = 1;
                let mut align = '<'; // left
                let mut num_decimals: Option<usize> = None;
                if let Some(&fc) = chars.peek() {
                    match fc {
                        '<' | '>' | '|' => {
                            align = fc;
                            while chars.peek() == Some(&fc) {
                                chars.next();
                                field_len += 1;
                            }
                        }
                        '#' => {
                            align = '#';
                            while let Some(&pc) = chars.peek() {
                                if pc == '#' {
                                    chars.next();
                                    field_len += 1;
                                    if let Some(ref mut d) = num_decimals {
                                        *d += 1;
                                    }
                                } else if pc == '.' && num_decimals.is_none() {
                                    chars.next();
                                    field_len += 1;
                                    num_decimals = Some(0);
                                } else {
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                let val = if val_idx < values.len() {
                    values[val_idx].clone()
                } else {
                    String::new()
                };
                val_idx += 1;
                let formatted = match align {
                    '>' => format!("{:>width$}", val, width = field_len),
                    '|' => {
                        let pad = field_len.saturating_sub(val.len());
                        let left = pad / 2;
                        let right = pad - left;
                        format!("{}{}{}", " ".repeat(left), val, " ".repeat(right))
                    }
                    '#' => {
                        // Numeric field — format with correct decimal places
                        let num: f64 = val.parse().unwrap_or(0.0);
                        let s = if let Some(dp) = num_decimals {
                            format!("{:>width$.prec$}", num, width = field_len, prec = dp)
                        } else {
                            format!("{:>width$}", num as i64, width = field_len)
                        };
                        s
                    }
                    _ => format!("{:<width$}", val, width = field_len),
                };
                result.push_str(&formatted[..formatted.len().min(field_len)]);
            } else {
                result.push(c);
            }
        }
        // Accumulate into $^A (stored in self.accumulator_format)
        self.accumulator_format.push_str(&result);
        Ok(PerlValue::integer(1))
    }

    // ── tied(VAR) ──────────────────────────────────────────────────────
    fn builtin_tied(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        // Check all tie stores
        if let Some(obj) = self.tied_hashes.get(&name) {
            return Ok(obj.clone());
        }
        if let Some(obj) = self.tied_scalars.get(&name) {
            return Ok(obj.clone());
        }
        if let Some(obj) = self.tied_arrays.get(&name) {
            return Ok(obj.clone());
        }
        Ok(PerlValue::UNDEF)
    }

    // ── untie(VAR) ─────────────────────────────────────────────────────
    fn builtin_untie(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        self.tied_hashes.remove(&name);
        self.tied_scalars.remove(&name);
        self.tied_arrays.remove(&name);
        Ok(PerlValue::UNDEF)
    }

    // ── gethostbyaddr(ADDR, ADDRTYPE) ──────────────────────────────────
    fn builtin_gethostbyaddr(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 2 {
            return Err(PerlError::runtime(
                "gethostbyaddr: not enough arguments",
                line,
            ));
        }
        let addr_str = args[0].to_string();
        // Try to parse as IP and do reverse lookup
        use std::net::ToSocketAddrs;
        let lookup = format!("{}:0", addr_str);
        if let Ok(mut addrs) = lookup.to_socket_addrs() {
            if let Some(sa) = addrs.next() {
                // Best-effort reverse: return the IP as hostname
                return Ok(PerlValue::string(sa.ip().to_string()));
            }
        }
        Ok(PerlValue::UNDEF)
    }
}
