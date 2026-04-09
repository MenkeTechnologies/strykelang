//! Builtins dispatched from `FuncCall` (names not modeled as dedicated `ExprKind`s).
//! I/O uses `Interpreter::io_file_slots` for raw `read`/`write`/`seek` alongside buffered handles.

/// TCP/UDP socket storage (high-level `std::net`, not raw POSIX).
pub(crate) enum PerlSocket {
    Listener(TcpListener),
    Stream(TcpStream),
    #[allow(dead_code)]
    Udp(UdpSocket),
}

use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Shutdown, TcpListener, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

use crate::error::{PerlError, PerlResult};
use crate::interpreter::Interpreter;
use crate::value::PerlValue;

/// If `name` is a known builtin, evaluate and return `Some`. Otherwise `None` (try user sub).
pub(crate) fn try_builtin(
    interp: &mut Interpreter,
    name: &str,
    args: &[PerlValue],
    line: usize,
) -> Option<PerlResult<PerlValue>> {
    match name {
        "prototype" => Some(builtin_prototype(args)),
        "binmode" => Some(interp.builtin_binmode(args, line)),
        "fileno" => Some(interp.builtin_fileno(args, line)),
        "flock" => Some(interp.builtin_flock(args, line)),
        "getc" => Some(interp.builtin_getc(args, line)),
        "sysread" => Some(interp.builtin_sysread(args, line)),
        "syswrite" => Some(interp.builtin_syswrite(args, line)),
        "sysseek" => Some(interp.builtin_sysseek(args, line)),
        "truncate" => Some(interp.builtin_truncate(args, line)),
        "select" => Some(interp.builtin_select(args, line)),
        "fork" => Some(builtin_fork()),
        "wait" => Some(builtin_wait()),
        "waitpid" => Some(builtin_waitpid(args)),
        "kill" => Some(builtin_kill(args)),
        "alarm" => Some(builtin_alarm(args)),
        "sleep" => Some(builtin_sleep(args)),
        "times" => Some(builtin_times()),
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
        _ => None,
    }
}

fn builtin_csv_read(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::csv_read(&path)
}

fn builtin_csv_write(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "csv_write needs path and row list",
            0,
        ));
    }
    let path = args[0].to_string();
    if args.len() == 2 {
        match &args[1] {
            PerlValue::Array(a) => return crate::native_data::csv_write(&path, a),
            PerlValue::ArrayRef(r) => {
                let g = r.read();
                return crate::native_data::csv_write(
                    &path,
                    &g.iter().cloned().collect::<Vec<_>>(),
                );
            }
            PerlValue::Hash(h) => {
                return crate::native_data::csv_write(&path, &[PerlValue::Hash(h.clone())]);
            }
            PerlValue::HashRef(r) => {
                let g = r.read();
                return crate::native_data::csv_write(&path, &[PerlValue::Hash(g.clone())]);
            }
            _ => {}
        }
    }
    crate::native_data::csv_write(&path, &args[1..])
}

fn builtin_sqlite(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path = args.first().map(|v| v.to_string()).unwrap_or_default();
    crate::native_data::sqlite_open(&path)
}

fn builtin_quotemeta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::String(regex::escape(&s)))
}

fn builtin_prototype(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::Undef);
    }
    match &args[0] {
        PerlValue::CodeRef(sub) => Ok(PerlValue::String(sub.prototype.clone().unwrap_or_default())),
        _ => Ok(PerlValue::Undef),
    }
}

#[cfg(unix)]
fn builtin_fork() -> PerlResult<PerlValue> {
    let pid = unsafe { libc::fork() };
    Ok(PerlValue::Integer(pid as i64))
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
    Ok(PerlValue::Integer(pid as i64))
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
    Ok(PerlValue::Integer(r as i64))
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
        return Ok(PerlValue::Integer(0));
    }
    let pid = args[0].to_int() as libc::pid_t;
    let sig = args[1].to_int() as libc::c_int;
    let r = unsafe { libc::kill(pid, sig) };
    Ok(PerlValue::Integer(r as i64))
}

#[cfg(not(unix))]
fn builtin_kill(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::Integer(0))
}

fn builtin_alarm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sec = args.first().map(|v| v.to_int().max(0) as u32).unwrap_or(0);
    #[cfg(unix)]
    {
        let prev = unsafe { libc::alarm(sec) };
        Ok(PerlValue::Integer(prev as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = sec;
        Ok(PerlValue::Integer(0))
    }
}

fn builtin_sleep(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let secs = args.first().map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs_f64(secs));
    Ok(PerlValue::Integer(start.elapsed().as_secs() as i64))
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
        Ok(PerlValue::Array(vec![
            PerlValue::Float(user),
            PerlValue::Float(system),
            PerlValue::Float(cuser),
            PerlValue::Float(csystem),
        ]))
    }
    #[cfg(not(unix))]
    {
        Ok(PerlValue::Array(vec![
            PerlValue::Float(0.0),
            PerlValue::Float(0.0),
            PerlValue::Float(0.0),
            PerlValue::Float(0.0),
        ]))
    }
}

impl Interpreter {
    fn builtin_binmode(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let _ = (args, line);
        // Layer selection (`:utf8`) is a no-op; real binmode is platform-specific.
        Ok(PerlValue::Integer(1))
    }

    fn builtin_fileno(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        #[cfg(unix)]
        {
            if let Some(f) = self.io_file_slots.get(&name) {
                return Ok(PerlValue::Integer(f.as_raw_fd() as i64));
            }
            match name.as_str() {
                "STDIN" => Ok(PerlValue::Integer(0)),
                "STDOUT" => Ok(PerlValue::Integer(1)),
                "STDERR" => Ok(PerlValue::Integer(2)),
                _ => Ok(PerlValue::Integer(-1)),
            }
        }
        #[cfg(not(unix))]
        {
            match name.as_str() {
                "STDIN" | "STDOUT" | "STDERR" => Ok(PerlValue::Integer(0)),
                _ => Ok(PerlValue::Integer(-1)),
            }
        }
    }

    fn builtin_flock(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let name = args.first().map(|v| v.to_string()).unwrap_or_default();
        let op = args.get(1).map(|v| v.to_int()).unwrap_or(0);
        #[cfg(unix)]
        {
            if let Some(f) = self.io_file_slots.get(&name) {
                let fd = f.as_raw_fd();
                let lock_op = match op {
                    1 => libc::LOCK_SH,
                    2 => libc::LOCK_EX,
                    4 => libc::LOCK_NB | libc::LOCK_EX,
                    8 => libc::LOCK_UN,
                    _ => libc::LOCK_EX,
                };
                let r = unsafe { libc::flock(fd, lock_op) };
                return Ok(PerlValue::Integer(if r == 0 { 1 } else { 0 }));
            }
        }
        let _ = line;
        Ok(PerlValue::Integer(1))
    }

    fn builtin_getc(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        let name = args
            .first()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "STDIN".to_string());
        let mut buf = [0u8; 1];
        if name == "STDIN" {
            match std::io::stdin().read(&mut buf) {
                Ok(0) => return Ok(PerlValue::Undef),
                Ok(_) => {
                    return Ok(PerlValue::String(
                        String::from_utf8_lossy(&buf).into_owned(),
                    ))
                }
                Err(e) => {
                    self.errno = e.to_string();
                    return Ok(PerlValue::Undef);
                }
            }
        }
        if let Some(f) = self.io_file_slots.get_mut(&name) {
            match f.read(&mut buf) {
                Ok(0) => Ok(PerlValue::Undef),
                Ok(_) => Ok(PerlValue::String(
                    String::from_utf8_lossy(&buf).into_owned(),
                )),
                Err(e) => {
                    self.errno = e.to_string();
                    Ok(PerlValue::Undef)
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
        let n = if let Some(f) = self.io_file_slots.get_mut(&fh) {
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
        // Perl binds to scalar buffer — we only support returning bytes as string for now.
        let _s = String::from_utf8_lossy(&buf[..n]).into_owned();
        Ok(PerlValue::Integer(n as i64))
    }

    fn builtin_syswrite(&mut self, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime("syswrite: not enough arguments", line));
        }
        let fh = args[0].to_string();
        let data = args[1].to_string();
        let len = args[2].to_int().max(0) as usize;
        let chunk = &data.as_bytes()[..len.min(data.len())];
        if let Some(f) = self.io_file_slots.get_mut(&fh) {
            let n = f.write(chunk).unwrap_or(0);
            let _ = f.flush();
            return Ok(PerlValue::Integer(n as i64));
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
        if let Some(f) = self.io_file_slots.get_mut(&fh) {
            let w = match whence {
                0 => SeekFrom::Start(pos as u64),
                1 => SeekFrom::Current(pos),
                2 => SeekFrom::End(pos),
                _ => SeekFrom::Start(pos as u64),
            };
            match f.seek(w) {
                Ok(p) => Ok(PerlValue::Integer(p as i64)),
                Err(e) => {
                    self.errno = e.to_string();
                    Ok(PerlValue::Integer(-1))
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
                Ok(()) => Ok(PerlValue::Integer(1)),
                Err(e) => {
                    self.errno = e.to_string();
                    Ok(PerlValue::Integer(0))
                }
            },
            Err(e) => {
                self.errno = e.to_string();
                Ok(PerlValue::Integer(0))
            }
        }
    }

    fn builtin_select(&mut self, args: &[PerlValue], _line: usize) -> PerlResult<PerlValue> {
        // Four-arg select(RB, WB, EB, timeout): sleep for timeout seconds (best-effort).
        if args.len() >= 4 {
            let t = args[3].to_number().max(0.0);
            std::thread::sleep(Duration::from_secs_f64(t));
            return Ok(PerlValue::Integer(0));
        }
        // One-arg: set default output handle (no-op; return previous "main").
        if args.len() == 1 {
            return Ok(PerlValue::String("main::STDOUT".into()));
        }
        Ok(PerlValue::Integer(0))
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
                Ok(PerlValue::Integer(1))
            }
            Err(e) => {
                self.errno = e;
                Ok(PerlValue::Integer(0))
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
                Ok(PerlValue::Integer(1))
            }
            Err(e) => {
                self.errno = e.to_string();
                Ok(PerlValue::Integer(0))
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
            return Ok(PerlValue::Integer(1));
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
                    Ok(PerlValue::Integer(1))
                }
                Err(e) => {
                    self.errno = e.to_string();
                    Ok(PerlValue::Integer(0))
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
                Ok(PerlValue::Integer(1))
            }
            Err(e) => {
                self.errno = e.to_string();
                Ok(PerlValue::Integer(0))
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
            return Ok(PerlValue::Integer(n as i64));
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
            return Ok(PerlValue::String(
                String::from_utf8_lossy(&buf[..n]).into_owned(),
            ));
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
            return Ok(PerlValue::Integer(1));
        }
        Err(PerlError::runtime("shutdown: not a stream socket", line))
    }
}
