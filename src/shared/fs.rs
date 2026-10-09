//! Rust translation of `libshared src/FS.cpp` (everything declared in
//! `FS.h`): the `Path`, `File` and `Directory` classes.
//!
//! # Shape
//!
//! The C++ hierarchy `Directory : File : Path` is modelled by composition
//! plus [`Deref`]/[`DerefMut`]: a [`Directory`] wraps a [`File`], which wraps
//! a [`Path`]. Every `Path` method is therefore callable on a `File` or a
//! `Directory`, and every `File` method on a `Directory`, exactly as in C++.
//! The C++ virtual overrides (`create`, `remove`) are inherent methods of the
//! derived type, which Rust's method resolution picks before the deref target.
//!
//! The C++ public members `_original` and `_data` are the public fields
//! [`Path::original`] (the text as given) and [`Path::data`] (the text after
//! [`Path::expand`]).
//!
//! | C++                                   | Rust                                          |
//! |---------------------------------------|-----------------------------------------------|
//! | `Path ()`, `Path (const std::string&)`| [`Path::default`], [`Path::new`] (also `From<&str>`/`From<String>`) |
//! | `Path (const Path&)`, `operator=`     | [`Clone`]                                     |
//! | `operator==`                          | [`PartialEq`] (compares `data` only)          |
//! | `operator+=`                          | `AddAssign<&str>`                               |
//! | `operator std::string`                | [`Display`](fmt::Display) / `String::from(&path)` |
//! | `File (const Path&)` etc.             | [`File::new`], `From<Path>`, `From<&Path>`, [`Clone`] |
//! | `File::read (std::string&)`           | [`File::read`] (returns the text)            |
//! | `File::read (std::vector<std::string>&)` | [`File::read_lines`]                      |
//! | `File::append (vector)`               | [`File::append_lines`]                        |
//! | static `File::create/read/write/remove/move` | [`File::create_file`], [`File::read_file`], [`File::read_file_lines`], [`File::write_file`], [`File::write_file_lines`], [`File::remove_file`], [`File::move_file`] |
//! | static `File::copy`, `File::removeBOM`| [`File::copy`], [`File::remove_bom`]          |
//! | `File& operator= (const File&)`       | [`File::assign`] (plain Rust assignment replaces the whole value) |
//! | `Directory::listRecursive`            | [`Directory::list_recursive`]                 |
//!
//! The C++ default arguments become explicit parameters; the defaults are
//! [`File::DEFAULT_MODE`] (`0640`) and [`Directory::DEFAULT_MODE`] (`0755`).
//!
//! # Errors
//!
//! Every C++ `throw format ("<call> error {1}: {2}", errno, strerror (errno))`
//! is an `Err(TwError::Message)` with the same text, so those methods return
//! [`Result`]. Methods that never throw in C++ return plain values. C++ out
//! parameters of the `read` functions become return values: the instance
//! reads return the text, the static reads return `None` where C++ returns
//! `false` (C++ also leaves the out parameter empty then).
//!
//! # POSIX fidelity
//!
//! libc is used wherever std would differ from the C++ POSIX call:
//! `access (2)` for `exists`/`readable`/`writable`/`executable` (real-uid
//! permission checks, `EACCES` handling), `stat`/`lstat` fields for `mode`,
//! `size`, `mtime`, `ctime` and `btime`, `mkdir`/`chmod`/`fchmod` with an
//! explicit mode, `fcntl (F_SETLKW)` record locks, `fdatasync`, `glob (3)`
//! with `GLOB_ERR | GLOB_BRACE | GLOB_TILDE`, and `getpwuid_r`/`getpwnam_r`
//! for `~` and `~user` (the reentrant forms of the C++ `getpwuid`/`getpwnam`).
//! As with C++ `c_str ()`, a path containing a NUL byte is used only up to
//! that byte.
//!
//! Semantics preserved from C++, including its quirks:
//! - `read`/`read_lines` (instance and static) go line by line like
//!   `std::getline`: the text form ends every line, including an
//!   unterminated last line, with `'\n'`; a UTF-8 BOM on the first line is
//!   removed. The instance `read` stats the file first and so fails on a
//!   missing file; the static reads do not expand `~`/`$VAR`, while the
//!   static `create`/`write`/`remove`/`move` do.
//! - `append`/`write_raw` write up to the first NUL (C++ `fputs`), and
//!   `append_lines` adds no newlines. `write_file_lines` adds one after every
//!   line when asked to.
//! - [`File::copy`] reports `false` when the source is empty, because the
//!   C++ `dst << src.rdbuf ()` sets `failbit` when it inserts nothing (the
//!   empty destination is still created).
//! - [`File::move_file`] compares `from` with the expanded `to` but renames
//!   to the unexpanded `to`.
//! - [`File::unlock`] issues `F_SETLK` with `F_WRLCK`, as the C++ does, so
//!   the record lock is in fact released only when the file is closed.
//! - On Linux (where Taskwarrior defines `LINUX` and finds
//!   `get_current_dir_name`), [`File::close`] uses `fdatasync` and
//!   [`Directory::cwd`] returns `$PWD` when it names the current directory.
//!   `btime` is the birth time only where `struct stat` has `st_birthtime`
//!   (macOS and the BSDs); elsewhere it is `ctime`, as in C++.
//!
//! Deviations, each limited to cases that are broken or undefined in C++:
//! - File names are Rust `String`s, so a name read from the file system
//!   (`list`, `glob`, `realpath`, `cwd`) that is not valid UTF-8 is converted
//!   lossily. File contents read as text are converted the same way.
//! - [`Directory::create`] does not recurse into an empty parent (the parent
//!   of `"/top"` or of `""`); C++ recurses on it forever.
//! - Where C++ dereferences a null `getpwuid` result for `~`, nothing is
//!   substituted; where it would throw `std::bad_alloc` or read an
//!   uninitialised buffer in `cwd`, an empty string is returned.
//! - The `_WIN32` branches are not ported: this translation is Unix-only.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

// libc field and constant types (`mode_t`, `time_t`, `off_t`, `size_t`)
// differ between platforms, so casts that are no-ops on one are needed on
// another.
#![allow(clippy::unnecessary_cast)]

use std::ffi::{CStr, CString};
use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::{AddAssign, Deref, DerefMut};
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, IntoRawFd};

use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::shared::shared::strerror;

////////////////////////////////////////////////////////////////////////////////
// POSIX helpers.

/// `GLOB_BRACE`/`GLOB_TILDE` where the platform glob supports them; C++
/// defines them as 0 where they are missing (musl).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const GLOB_FLAGS: c_int = libc::GLOB_ERR | libc::GLOB_BRACE | libc::GLOB_TILDE;
#[cfg(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
const GLOB_FLAGS: c_int = libc::GLOB_ERR | 0x0080 /* GLOB_BRACE */ | 0x0800 /* GLOB_TILDE */;
#[cfg(not(any(
    all(target_os = "linux", target_env = "gnu"),
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
const GLOB_FLAGS: c_int = libc::GLOB_ERR;

/// The part of `text` C++ `c_str ()` passes to a C call: everything before
/// the first NUL.
fn c_str(text: &str) -> &str {
    match text.find('\0') {
        Some(nul) => &text[..nul],
        None => text,
    }
}

/// `text` as a C string, truncated at the first NUL like `c_str ()`.
fn c_string(text: &str) -> CString {
    CString::new(c_str(text)).unwrap_or_default()
}

/// The calling thread's `errno`.
fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// C++ `format ("<call> error {1}: {2}", errno, strerror (errno))`.
fn os_error(call: &str, code: i32) -> TwError {
    TwError::Message(format!("{call} error {code}: {}", strerror(code)))
}

/// [`os_error`] for a failed std I/O call.
fn io_error(call: &str, err: &std::io::Error) -> TwError {
    os_error(call, err.raw_os_error().unwrap_or(0))
}

/// Lossy `String` from raw bytes (file names and contents).
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// C `access (path, how) == 0`, and the `errno` when it is not.
fn access(path: &str, how: c_int) -> std::result::Result<(), i32> {
    let path = c_string(path);
    // SAFETY: `path` is a valid NUL-terminated string.
    if unsafe { libc::access(path.as_ptr(), how) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

/// C `stat` (`follow = true`) or `lstat`, raising C++'s error text.
fn stat_path(path: &str, follow: bool) -> Result<libc::stat> {
    let c_path = c_string(path);
    // SAFETY: an all-zero `struct stat` is a valid value to be overwritten.
    let mut s: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is NUL-terminated and `s` is a valid out pointer.
    let status = unsafe {
        if follow {
            libc::stat(c_path.as_ptr(), &mut s)
        } else {
            libc::lstat(c_path.as_ptr(), &mut s)
        }
    };
    if status != 0 {
        return Err(os_error(if follow { "stat" } else { "lstat" }, errno()));
    }

    Ok(s)
}

/// C `S_ISDIR`/`S_ISLNK`/...: whether `mode` has file type `kind`.
fn is_type(mode: libc::mode_t, kind: libc::mode_t) -> bool {
    (mode & libc::S_IFMT) == kind
}

/// `pw_dir` of the passwd entry a `getpw*_r` call yields, if any.
fn passwd_dir(
    lookup: impl Fn(*mut libc::passwd, *mut c_char, libc::size_t, *mut *mut libc::passwd) -> c_int,
) -> Option<Vec<u8>> {
    let mut size = 16 * 1024;
    loop {
        let mut buf: Vec<c_char> = vec![0; size];
        // SAFETY: an all-zero `struct passwd` is a valid out value.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let status = lookup(&mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if status == libc::ERANGE && size < 1024 * 1024 {
            size *= 2;
            continue;
        }
        if status != 0 || result.is_null() || pwd.pw_dir.is_null() {
            return None;
        }
        // SAFETY: on success `pw_dir` points to a NUL-terminated string in `buf`.
        return Some(unsafe { CStr::from_ptr(pwd.pw_dir) }.to_bytes().to_vec());
    }
}

/// C++ `getpwuid (getuid ())->pw_dir`.
fn home_of_current_user() -> Option<Vec<u8>> {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    passwd_dir(|pwd, buf, len, result| {
        // SAFETY: all pointers are valid and `buf` holds `len` bytes.
        unsafe { libc::getpwuid_r(uid, pwd, buf, len, result) }
    })
}

/// C++ `getpwnam (name)->pw_dir`, or `None` when there is no such user.
fn home_of_user(name: &str) -> Option<Vec<u8>> {
    let name = c_string(name);
    passwd_dir(|pwd, buf, len, result| {
        // SAFETY: all pointers are valid and `buf` holds `len` bytes.
        unsafe { libc::getpwnam_r(name.as_ptr(), pwd, buf, len, result) }
    })
}

/// C++ `std::getline` over the whole of `bytes`, removing a BOM from the
/// first line (the shared loop of every `File::read`).
fn getline_all(bytes: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    if bytes.is_empty() {
        return lines;
    }

    let mut pieces: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    // A terminating newline does not start another line.
    if pieces.last().is_some_and(|last| last.is_empty()) {
        pieces.pop();
    }

    for (index, piece) in pieces.into_iter().enumerate() {
        let piece = if index == 0 {
            piece.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(piece)
        } else {
            piece
        };
        lines.push(lossy(piece));
    }

    lines
}

/// Opens `name` for reading like a C++ `std::ifstream` and returns its lines,
/// or `None` when it cannot be opened (`in.good ()` is false).
fn ifstream_lines(name: &str) -> Option<Vec<String>> {
    let mut file = std::fs::File::open(c_str(name)).ok()?;
    let mut bytes = Vec::new();
    // As in C++, a read error ends the input; what was read is kept.
    let _ = file.read_to_end(&mut bytes);
    Some(getline_all(&bytes))
}

/// The C++ text form of [`File::read`]: every line followed by `'\n'`.
fn join_lines(lines: &[String]) -> String {
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }

    contents
}

/// Opens `name` like a C++ `std::ofstream (name, out | trunc)`.
fn ofstream(name: &str) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(c_str(name))
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `Path`: a file system path, kept both as given (`original`) and
/// expanded (`data`).
#[derive(Debug, Clone, Default)]
pub struct Path {
    /// C++ `_original`: the path as given to the constructor.
    pub original: String,
    /// C++ `_data`: the path after [`Path::expand`].
    pub data: String,
}

impl Path {
    /// C++ `Path::Path (const std::string&)`: keeps `input` as the original
    /// and its [`expand`](Path::expand)ed form as the data.
    pub fn new(input: &str) -> Self {
        Path {
            original: input.to_string(),
            data: Path::expand(input),
        }
    }

    /// C++ `Path::name`: the text after the last `/` (all of it if none).
    pub fn name(&self) -> String {
        match self.data.rfind('/') {
            Some(slash) => self.data[slash + 1..].to_string(),
            None => self.data.clone(),
        }
    }

    /// C++ `Path::parent`: the text before the last `/`, `"."` if there is
    /// none, and `""` for an empty path.
    pub fn parent(&self) -> String {
        if self.data.is_empty() {
            return String::new();
        }

        match self.data.rfind('/') {
            Some(slash) => self.data[..slash].to_string(),
            None => ".".to_string(),
        }
    }

    /// C++ `Path::extension`: the text after the last `.`, or `""`.
    pub fn extension(&self) -> String {
        match self.data.rfind('.') {
            Some(dot) => self.data[dot + 1..].to_string(),
            None => String::new(),
        }
    }

    /// C++ `Path::realpath`: the canonical absolute path (`realpath (3)`), or
    /// `""` when the path is empty or cannot be resolved.
    pub fn realpath(&self) -> String {
        if self.data.is_empty() {
            return String::new();
        }

        match std::fs::canonicalize(c_str(&self.data)) {
            Ok(resolved) => lossy(resolved.as_os_str().as_bytes()),
            Err(_) => String::new(),
        }
    }

    /// C++ `Path::exists`: `access (F_OK) == 0`.
    pub fn exists(&self) -> bool {
        access(&self.data, libc::F_OK).is_ok()
    }

    /// C++ `Path::is_directory`: whether the path exists and (following
    /// symlinks) is a directory.
    pub fn is_directory(&self) -> Result<bool> {
        if self.exists() {
            let s = stat_path(&self.data, true)?;
            return Ok(is_type(s.st_mode, libc::S_IFDIR));
        }

        Ok(false)
    }

    /// C++ `Path::is_absolute`: whether the path starts with `/`.
    pub fn is_absolute(&self) -> bool {
        self.data.starts_with('/')
    }

    /// C++ `Path::is_link`: whether the path exists and is itself a
    /// symbolic link (`lstat`).
    pub fn is_link(&self) -> Result<bool> {
        if !self.exists() {
            return Ok(false);
        }

        let s = stat_path(&self.data, false)?;
        Ok(is_type(s.st_mode, libc::S_IFLNK))
    }

    /// Shared body of `readable`/`writable`/`executable`: `EACCES` is the
    /// answer being sought, any other error is raised.
    fn accessible(&self, how: c_int) -> Result<bool> {
        if !self.exists() {
            return Ok(false);
        }

        match access(&self.data, how) {
            Ok(()) => Ok(true),
            Err(code) if code == libc::EACCES => Ok(false),
            Err(code) => Err(os_error("access", code)),
        }
    }

    /// C++ `Path::readable`: `access (R_OK)`.
    pub fn readable(&self) -> Result<bool> {
        self.accessible(libc::R_OK)
    }

    /// C++ `Path::writable`: `access (W_OK)`.
    pub fn writable(&self) -> Result<bool> {
        self.accessible(libc::W_OK)
    }

    /// C++ `Path::executable`: `access (X_OK)`.
    pub fn executable(&self) -> Result<bool> {
        self.accessible(libc::X_OK)
    }

    /// C++ `Path::rename`: renames the file to the expanded `new_name` and
    /// stores that as the data. Renaming to the same path is `false`.
    pub fn rename(&mut self, new_name: &str) -> bool {
        let expanded = Path::expand(new_name);
        if self.data != expanded {
            let from = c_string(&self.data);
            let to = c_string(&expanded);
            // SAFETY: both arguments are valid NUL-terminated strings.
            if unsafe { libc::rename(from.as_ptr(), to.as_ptr()) } == 0 {
                self.data = expanded;
                return true;
            }
        }

        false
    }

    /// C++ static `Path::expand`. Supported expansion:
    /// - tilde `~` at the start → home directory of the current user
    ///   (`$HOME`, else the passwd entry);
    /// - tilde `~foo` at the start → home directory of user `foo` (the
    ///   passwd entry, else `/home/foo`);
    /// - environment `$FOO` → value of `FOO` (empty if unset); a `$` not
    ///   followed by a letter, digit or `_` is kept.
    ///
    /// A relative path stays relative.
    pub fn expand(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut result: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            // Expand `~` only at start.
            if i == 0 && bytes[0] == b'~' {
                let slash = bytes[1..]
                    .iter()
                    .position(|&b| b == b'/')
                    .map_or(bytes.len(), |p| p + 1);

                if slash == 1 {
                    // Convert: ~ --> /home/user
                    match std::env::var_os("HOME") {
                        Some(home) => result.extend_from_slice(home.as_bytes()),
                        None => {
                            if let Some(home) = home_of_current_user() {
                                result.extend_from_slice(&home);
                            }
                        }
                    }
                } else {
                    // Convert: ~name --> /home/name
                    let name = &input[1..slash];
                    match home_of_user(name) {
                        Some(home) => result.extend_from_slice(&home),
                        None => {
                            result.extend_from_slice(b"/home/");
                            result.extend_from_slice(name.as_bytes());
                        }
                    }
                }

                // Process `/` in the next loop.
                i = slash;
            }
            // Expand environment variable.
            else if bytes[i] == b'$' {
                let mut end = i + 1;
                while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }

                if i + 1 == end {
                    result.push(b'$');
                } else if let Some(value) = std::env::var_os(&input[i + 1..end]) {
                    result.extend_from_slice(value.as_bytes());
                }

                i = end;
            }
            // Otherwise, keep the char.
            else {
                result.push(bytes[i]);
                i += 1;
            }
        }

        lossy(&result)
    }

    /// C++ static `Path::glob`: the matches of `pattern` from `glob (3)`
    /// with `GLOB_ERR | GLOB_BRACE | GLOB_TILDE`, empty on no match or error.
    pub fn glob(pattern: &str) -> Vec<String> {
        let mut results = Vec::new();
        let pattern = c_string(pattern);
        // SAFETY: an all-zero `glob_t` is the documented initial state.
        let mut g: libc::glob_t = unsafe { std::mem::zeroed() };
        // SAFETY: `pattern` is NUL-terminated and `g` is a valid glob_t.
        if unsafe { libc::glob(pattern.as_ptr(), GLOB_FLAGS, None, &mut g) } == 0 {
            for i in 0..g.gl_pathc as usize {
                // SAFETY: glob filled `gl_pathv` with `gl_pathc` C strings.
                let entry = unsafe { CStr::from_ptr(*g.gl_pathv.add(i)) };
                results.push(lossy(entry.to_bytes()));
            }
        }

        // SAFETY: `g` was initialised by glob (or is all zero).
        unsafe { libc::globfree(&mut g) };
        results
    }
}

/// C++ `Path::operator==`: compares the expanded data only.
impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for Path {}

/// C++ `Path::operator+=`: appends `/` and `dir` to the data.
impl AddAssign<&str> for Path {
    fn add_assign(&mut self, dir: &str) {
        self.data.push('/');
        self.data.push_str(dir);
    }
}

/// C++ `Path::operator std::string`: the expanded data.
impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.data)
    }
}

impl From<&Path> for String {
    fn from(path: &Path) -> Self {
        path.data.clone()
    }
}

impl From<&str> for Path {
    fn from(input: &str) -> Self {
        Path::new(input)
    }
}

impl From<String> for Path {
    fn from(input: String) -> Self {
        Path::new(&input)
    }
}

impl From<&String> for Path {
    fn from(input: &String) -> Self {
        Path::new(input)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `File : Path`: a path plus an optional open handle (C++ `_fh`/`_h`)
/// and a record-lock flag.
///
/// The handle is unbuffered, so data written by [`append`](File::append) or
/// [`write_raw`](File::write_raw) reaches the file at once instead of when
/// the C++ `FILE*` buffer is flushed; the bytes and their order are the same.
#[derive(Debug, Default)]
pub struct File {
    path: Path,
    fh: Option<std::fs::File>,
    locked: bool,
}

impl File {
    /// C++ default `mode` of `File::create`.
    pub const DEFAULT_MODE: u32 = 0o640;

    /// C++ `File::File (const std::string&)`.
    pub fn new(input: &str) -> Self {
        File::from(Path::new(input))
    }

    /// C++ `File& operator= (const File&)`: copies the path and clears the
    /// lock flag but keeps this object's open handle, as C++ does.
    pub fn assign(&mut self, other: &Path) {
        self.path = other.clone();
        self.locked = false;
    }

    /// C++ virtual `File::create (int mode = 0640)`: opens (creating if
    /// needed, never truncating), sets `mode` with `fchmod`, and closes.
    pub fn create(&mut self, mode: u32) -> Result<bool> {
        if self.open()? {
            if let Some(fh) = &self.fh {
                // SAFETY: `fh` is an open descriptor. Errors are ignored, as in C++.
                unsafe { libc::fchmod(fh.as_raw_fd(), mode as libc::mode_t) };
            }
            self.close()?;
            return Ok(true);
        }

        Ok(false)
    }

    /// C++ virtual `File::remove`: `unlink (2)`.
    pub fn remove(&self) -> bool {
        let path = c_string(&self.path.data);
        // SAFETY: `path` is a valid NUL-terminated string.
        unsafe { libc::unlink(path.as_ptr()) == 0 }
    }

    /// C++ static `File::removeBOM`: `input` without a leading UTF-8 BOM.
    pub fn remove_bom(input: &str) -> String {
        input.strip_prefix('\u{FEFF}').unwrap_or(input).to_string()
    }

    /// C++ `File::open`: opens for reading and writing (`"r+"` if the file
    /// exists, otherwise `"w+"`, which creates it). `false` for an empty
    /// path; `true` if already open. An existing file that is not both
    /// readable and writable is an error.
    pub fn open(&mut self) -> Result<bool> {
        if self.path.data.is_empty() {
            return Ok(false);
        }

        if self.fh.is_some() {
            return Ok(true);
        }

        let already_exists = self.exists();
        if already_exists && (!self.readable()? || !self.writable()?) {
            return Err(TwError::Message(format(
                "Insufficient permissions for '{1}'.",
                &[&self.path.data],
            )));
        }

        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true);
        if !already_exists {
            options.create(true).truncate(true);
        }

        match options.open(c_str(&self.path.data)) {
            Ok(fh) => {
                self.fh = Some(fh);
                self.locked = false;
                Ok(true)
            }
            Err(err) => Err(io_error("fopen", &err)),
        }
    }

    /// C++ `File::close`: unlocks, flushes to the device (`fdatasync` on
    /// Linux, `F_FULLFSYNC` falling back to `fsync` on macOS, `fsync`
    /// elsewhere) and closes. A no-op when not open.
    pub fn close(&mut self) -> Result<()> {
        if self.fh.is_none() {
            return Ok(());
        }

        if self.locked {
            self.unlock();
        }

        let fd = self.fh.as_ref().map_or(-1, |fh| fh.as_raw_fd());
        sync_descriptor(fd)?;

        if let Some(fh) = self.fh.take() {
            let fd = fh.into_raw_fd();
            // SAFETY: `fd` was just released by its owner and is closed once.
            if unsafe { libc::close(fd) } != 0 {
                return Err(os_error("fclose", errno()));
            }
        }

        self.locked = false;
        Ok(())
    }

    /// `fcntl` with a whole-file `F_WRLCK` record lock, as both C++ `lock`
    /// and `unlock` build it.
    fn fcntl_write_lock(&self, command: c_int) -> bool {
        let Some(fh) = &self.fh else {
            return false;
        };

        // SAFETY: an all-zero `struct flock` is valid; fields are set below.
        let mut fl: libc::flock = unsafe { std::mem::zeroed() };
        fl.l_type = libc::F_WRLCK as _;
        fl.l_whence = libc::SEEK_SET as _;
        fl.l_start = 0;
        fl.l_len = 0;
        // SAFETY: getpid cannot fail.
        fl.l_pid = unsafe { libc::getpid() };
        // SAFETY: `fh` is an open descriptor and `fl` a valid flock.
        unsafe { libc::fcntl(fh.as_raw_fd(), command, &fl) == 0 }
    }

    /// C++ `File::lock`: waits for an exclusive record lock on the whole
    /// open file (`F_SETLKW`). `false` when not open or on failure.
    pub fn lock(&mut self) -> bool {
        self.locked = false;
        if self.fh.is_some() && self.fcntl_write_lock(libc::F_SETLKW) {
            self.locked = true;
        }

        self.locked
    }

    /// C++ `File::unlock`. Like the C++, this issues `F_SETLK` with
    /// `F_WRLCK` (not `F_UNLCK`), so the lock itself lasts until close.
    pub fn unlock(&mut self) {
        if self.locked {
            self.fcntl_write_lock(libc::F_SETLK);
            self.locked = false;
        }
    }

    /// C++ `File::read (std::string&)`: the whole file, every line ending
    /// with `'\n'` and a first-line BOM removed. Fails, as C++ does, when the
    /// file cannot be `stat`ed (C++ reserves `size ()` bytes first); an
    /// unreadable file gives `""`.
    pub fn read(&self) -> Result<String> {
        let mut contents = String::new();
        contents.reserve(self.size()?);
        if let Some(lines) = ifstream_lines(&self.path.data) {
            contents.push_str(&join_lines(&lines));
        }

        Ok(contents)
    }

    /// C++ `File::read (std::vector<std::string>&)`: the lines of the file
    /// without their newlines, a first-line BOM removed; empty if the file
    /// cannot be read.
    pub fn read_lines(&self) -> Vec<String> {
        ifstream_lines(&self.path.data).unwrap_or_default()
    }

    /// Opens if necessary, then writes `text` (up to any NUL, like `fputs`)
    /// at the handle's current position (`seek_end`: at the end).
    fn put(&mut self, texts: &[&str], seek_end: bool) -> Result<()> {
        if self.fh.is_none() {
            self.open()?;
        }

        if let Some(fh) = &mut self.fh {
            if seek_end {
                let _ = fh.seek(SeekFrom::End(0));
            }

            for text in texts {
                fh.write_all(c_str(text).as_bytes())
                    .map_err(|err| io_error("fputs", &err))?;
            }
        }

        Ok(())
    }

    /// C++ `File::append (const std::string&)`: opens if necessary and
    /// writes `line` at the end of the file. No newline is added.
    pub fn append(&mut self, line: &str) -> Result<()> {
        self.put(&[line], true)
    }

    /// C++ `File::append (const std::vector<std::string>&)`: opens if
    /// necessary and writes every line at the end of the file, adding no
    /// newlines.
    pub fn append_lines<S: AsRef<str>>(&mut self, lines: &[S]) -> Result<()> {
        let texts: Vec<&str> = lines.iter().map(AsRef::as_ref).collect();
        self.put(&texts, true)
    }

    /// C++ `File::write_raw`: opens if necessary and writes `line` at the
    /// current position (the start of a freshly opened file).
    pub fn write_raw(&mut self, line: &str) -> Result<()> {
        self.put(&[line], false)
    }

    /// C++ `File::truncate`: opens if necessary and truncates the file to
    /// zero length. The handle's position is not moved.
    pub fn truncate(&mut self) -> Result<()> {
        if self.fh.is_none() {
            self.open()?;
        }

        if let Some(fh) = &self.fh {
            fh.set_len(0).map_err(|err| io_error("ftruncate", &err))?;
        }

        Ok(())
    }

    /// C++ virtual `File::mode`: `st_mode` (file type and permission bits).
    pub fn mode(&self) -> Result<u32> {
        Ok(stat_path(&self.path.data, true)?.st_mode as u32)
    }

    /// C++ virtual `File::size`: `st_size` in bytes.
    pub fn size(&self) -> Result<usize> {
        Ok(stat_path(&self.path.data, true)?.st_size as usize)
    }

    /// C++ virtual `File::mtime`: `st_mtime`.
    pub fn mtime(&self) -> Result<i64> {
        Ok(stat_path(&self.path.data, true)?.st_mtime as i64)
    }

    /// C++ virtual `File::ctime`: `st_ctime`.
    pub fn ctime(&self) -> Result<i64> {
        Ok(stat_path(&self.path.data, true)?.st_ctime as i64)
    }

    /// C++ virtual `File::btime`: `st_birthtime` where `struct stat` has it
    /// (`HAVE_ST_BIRTHTIME`), otherwise `st_ctime`.
    pub fn btime(&self) -> Result<i64> {
        let s = stat_path(&self.path.data, true)?;
        #[cfg(any(target_vendor = "apple", target_os = "freebsd", target_os = "netbsd"))]
        let seconds = s.st_birthtime as i64;
        #[cfg(not(any(target_vendor = "apple", target_os = "freebsd", target_os = "netbsd")))]
        let seconds = s.st_ctime as i64;
        Ok(seconds)
    }

    /// C++ static `File::create (const std::string&, int mode = 0640)`:
    /// creates (truncating) the expanded `name` and `chmod`s it to `mode`.
    /// `false` if it cannot be created; a failing `chmod` is an error.
    pub fn create_file(name: &str, mode: u32) -> Result<bool> {
        let full_name = Path::expand(name);
        if ofstream(&full_name).is_ok() {
            let path = c_string(&full_name);
            // SAFETY: `path` is a valid NUL-terminated string.
            if unsafe { libc::chmod(path.as_ptr(), mode as libc::mode_t) } != 0 {
                return Err(os_error("chmod", errno()));
            }

            return Ok(true);
        }

        Ok(false)
    }

    /// C++ static `File::read (const std::string&, std::string&)`: the text
    /// of `name` (not expanded), every line ending with `'\n'` and a
    /// first-line BOM removed; `None` if it cannot be opened.
    pub fn read_file(name: &str) -> Option<String> {
        ifstream_lines(name).map(|lines| join_lines(&lines))
    }

    /// C++ static `File::read (const std::string&, std::vector<std::string>&)`:
    /// the lines of `name` (not expanded) without newlines, a first-line BOM
    /// removed; `None` if it cannot be opened.
    pub fn read_file_lines(name: &str) -> Option<Vec<String>> {
        ifstream_lines(name)
    }

    /// C++ static `File::write (const std::string&, const std::string&)`:
    /// replaces the contents of the expanded `name` with `contents`.
    pub fn write_file(name: &str, contents: &str) -> bool {
        match ofstream(&Path::expand(name)) {
            Ok(mut out) => out.write_all(contents.as_bytes()).is_ok(),
            Err(_) => false,
        }
    }

    /// C++ static `File::write (const std::string&, const
    /// std::vector<std::string>&, bool addNewlines = true)`: replaces the
    /// contents of the expanded `name` with `lines`, each followed by `'\n'`
    /// when `add_newlines` is set.
    pub fn write_file_lines<S: AsRef<str>>(name: &str, lines: &[S], add_newlines: bool) -> bool {
        let Ok(mut out) = ofstream(&Path::expand(name)) else {
            return false;
        };

        let mut contents = String::new();
        for line in lines {
            contents.push_str(line.as_ref());
            if add_newlines {
                contents.push('\n');
            }
        }

        out.write_all(contents.as_bytes()).is_ok()
    }

    /// C++ static `File::remove (const std::string&)`: unlinks the expanded
    /// `name`.
    pub fn remove_file(name: &str) -> bool {
        let path = c_string(&Path::expand(name));
        // SAFETY: `path` is a valid NUL-terminated string.
        unsafe { libc::unlink(path.as_ptr()) == 0 }
    }

    /// C++ static `File::copy`: copies `from` to `to` (neither expanded).
    /// `false` if `from` cannot be opened, the copy fails, or (C++ stream
    /// quirk) `from` is empty.
    pub fn copy(from: &str, to: &str) -> bool {
        let Ok(mut src) = std::fs::File::open(c_str(from)) else {
            return false;
        };

        let Ok(mut dst) = ofstream(to) else {
            return false;
        };

        matches!(std::io::copy(&mut src, &mut dst), Ok(copied) if copied > 0)
    }

    /// C++ static `File::move`: `rename (from, to)` unless `from` equals the
    /// expanded `to` (the rename itself uses `to` unexpanded, as in C++).
    pub fn move_file(from: &str, to: &str) -> bool {
        let expanded = Path::expand(to);
        if from != expanded {
            let from = c_string(from);
            let to = c_string(to);
            // SAFETY: both arguments are valid NUL-terminated strings.
            if unsafe { libc::rename(from.as_ptr(), to.as_ptr()) } == 0 {
                return true;
            }
        }

        false
    }
}

/// `fdatasync`/`fsync`/`F_FULLFSYNC` step of C++ `File::close`.
fn sync_descriptor(fd: c_int) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `fd` is an open descriptor.
        if unsafe { libc::fdatasync(fd) } != 0 {
            return Err(os_error("fdatasync", errno()));
        }
    }

    #[cfg(target_vendor = "apple")]
    {
        // SAFETY: `fd` is an open descriptor.
        if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC, 0) } != 0 {
            // Some filesystems, like smbfs, do not support F_FULLFSYNC.
            // SAFETY: `fd` is an open descriptor.
            if unsafe { libc::fsync(fd) } != 0 {
                return Err(os_error("fsync", errno()));
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    {
        // SAFETY: `fd` is an open descriptor.
        if unsafe { libc::fsync(fd) } != 0 {
            return Err(os_error("fsync", errno()));
        }
    }

    Ok(())
}

/// C++ `File (const File&)`: copies the path only; the copy is not open and
/// not locked.
impl Clone for File {
    fn clone(&self) -> Self {
        File::from(self.path.clone())
    }
}

/// C++ `File::~File`: closes an open handle, ignoring errors.
impl Drop for File {
    fn drop(&mut self) {
        if self.fh.is_some() {
            let _ = self.close();
        }
    }
}

impl Deref for File {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl DerefMut for File {
    fn deref_mut(&mut self) -> &mut Path {
        &mut self.path
    }
}

/// Inherited C++ `Path::operator==`.
impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl fmt::Display for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.fmt(f)
    }
}

impl From<&File> for String {
    fn from(file: &File) -> Self {
        file.path.data.clone()
    }
}

/// C++ `File::File (const Path&)`.
impl From<Path> for File {
    fn from(path: Path) -> Self {
        File {
            path,
            fh: None,
            locked: false,
        }
    }
}

impl From<&Path> for File {
    fn from(path: &Path) -> Self {
        File::from(path.clone())
    }
}

impl From<&str> for File {
    fn from(input: &str) -> Self {
        File::new(input)
    }
}

impl From<String> for File {
    fn from(input: String) -> Self {
        File::new(&input)
    }
}

impl From<&String> for File {
    fn from(input: &String) -> Self {
        File::new(input)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `Directory : File`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Directory {
    file: File,
}

impl Directory {
    /// C++ default `mode` of `Directory::create`.
    pub const DEFAULT_MODE: u32 = 0o755;

    /// C++ `Directory::Directory (const std::string&)`.
    pub fn new(input: &str) -> Self {
        Directory {
            file: File::new(input),
        }
    }

    /// C++ virtual `Directory::create (int mode = 0755)`: `mkdir -p` style,
    /// creating missing parents with the same `mode` first. Failures are
    /// silent (`false`). The mode is subject to the umask.
    pub fn create(&self, mode: u32) -> bool {
        // No error handling because we want failure to be silent, somewhat
        // emulating "mkdir -p".
        let parent_dir = Directory::new(&self.parent());
        // C++ recurses forever on an empty parent; stop there instead.
        if !parent_dir.data.is_empty() && !parent_dir.exists() && !parent_dir.create(mode) {
            return false;
        }

        let path = c_string(&self.data);
        // SAFETY: `path` is a valid NUL-terminated string.
        unsafe { libc::mkdir(path.as_ptr(), mode as libc::mode_t) == 0 }
    }

    /// C++ virtual `Directory::remove`: removes the directory and everything
    /// in it. Symbolic links are unlinked, not followed.
    pub fn remove(&self) -> Result<bool> {
        Directory::remove_directory(&self.data)
    }

    /// C++ private `Directory::remove_directory`.
    fn remove_directory(dir: &str) -> Result<bool> {
        if let Ok(entries) = std::fs::read_dir(c_str(dir)) {
            for entry in entries {
                let Ok(entry) = entry else { break };
                let child = format!("{dir}/{}", lossy(entry.file_name().as_bytes()));
                // d_type, or lstat when it is DT_UNKNOWN.
                let file_type = entry.file_type().map_err(|err| io_error("lstat", &err))?;
                if file_type.is_dir() {
                    Directory::remove_directory(&child)?;
                } else {
                    let path = c_string(&child);
                    // SAFETY: `path` is a valid NUL-terminated string.
                    unsafe { libc::unlink(path.as_ptr()) };
                }
            }
        }

        let path = c_string(dir);
        // SAFETY: `path` is a valid NUL-terminated string.
        Ok(unsafe { libc::rmdir(path.as_ptr()) } == 0)
    }

    /// C++ `Directory::list`: `<dir>/<name>` for every entry (files and
    /// directories, not `.`/`..`), in directory order.
    pub fn list(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        Directory::list_into(&self.data, &mut files, false)?;
        Ok(files)
    }

    /// C++ `Directory::listRecursive`: `<dir>/.../<name>` for every
    /// non-directory below the directory, descending into subdirectories
    /// (but not through symbolic links).
    pub fn list_recursive(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        Directory::list_into(&self.data, &mut files, true)?;
        Ok(files)
    }

    /// C++ private `Directory::list (base, results, recursive)`.
    fn list_into(base: &str, results: &mut Vec<String>, recursive: bool) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(c_str(base)) {
            for entry in entries {
                let Ok(entry) = entry else { break };
                let child = format!("{base}/{}", lossy(entry.file_name().as_bytes()));
                let is_dir = recursive
                    && entry
                        .file_type()
                        .map_err(|err| io_error("lstat", &err))?
                        .is_dir();
                if is_dir {
                    Directory::list_into(&child, results, recursive)?;
                } else {
                    results.push(child);
                }
            }
        }

        Ok(())
    }

    /// C++ static `Directory::cwd`: the current working directory. On Linux
    /// this is `get_current_dir_name (3)`, which prefers `$PWD` when it names
    /// the same directory. `""` if it cannot be determined.
    pub fn cwd() -> String {
        #[cfg(target_os = "linux")]
        if let Some(pwd) = std::env::var_os("PWD") {
            let pwd = lossy(pwd.as_bytes());
            if let (Ok(a), Ok(b)) = (stat_path(&pwd, true), stat_path(".", true)) {
                if a.st_dev == b.st_dev && a.st_ino == b.st_ino {
                    return pwd;
                }
            }
        }

        match std::env::current_dir() {
            Ok(dir) => lossy(dir.as_os_str().as_bytes()),
            Err(_) => String::new(),
        }
    }

    /// C++ `Directory::up`: replaces the data with its parent directory;
    /// `/a` goes to `/`. `false` (and no change) at `/` or when there is no
    /// `/` left.
    pub fn up(&mut self) -> bool {
        if self.data == "/" {
            return false;
        }

        match self.data.rfind('/') {
            Some(0) => {
                // Root dir should retain the slash.
                self.data = "/".to_string();
                true
            }
            Some(slash) => {
                self.data.truncate(slash);
                true
            }
            None => false,
        }
    }

    /// C++ `Directory::cd`: `chdir (2)` into the directory. This changes the
    /// working directory of the whole process.
    pub fn cd(&self) -> bool {
        let path = c_string(&self.data);
        // SAFETY: `path` is a valid NUL-terminated string.
        unsafe { libc::chdir(path.as_ptr()) == 0 }
    }
}

impl Deref for Directory {
    type Target = File;

    fn deref(&self) -> &File {
        &self.file
    }
}

impl DerefMut for Directory {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

impl fmt::Display for Directory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.file.fmt(f)
    }
}

impl From<&Directory> for String {
    fn from(dir: &Directory) -> Self {
        dir.data.clone()
    }
}

/// C++ `Directory::Directory (const File&)`: copies the path only.
impl From<&File> for Directory {
    fn from(file: &File) -> Self {
        Directory { file: file.clone() }
    }
}

impl From<File> for Directory {
    fn from(file: File) -> Self {
        Directory::from(&file)
    }
}

/// C++ `Directory::Directory (const Path&)`.
impl From<&Path> for Directory {
    fn from(path: &Path) -> Self {
        Directory {
            file: File::from(path),
        }
    }
}

impl From<Path> for Directory {
    fn from(path: Path) -> Self {
        Directory {
            file: File::from(path),
        }
    }
}

impl From<&str> for Directory {
    fn from(input: &str) -> Self {
        Directory::new(input)
    }
}

impl From<String> for Directory {
    fn from(input: String) -> Self {
        Directory::new(&input)
    }
}

impl From<&String> for Directory {
    fn from(input: &String) -> Self {
        Directory::new(input)
    }
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/fs.t.cpp` (128 assertions), then Rust-only
    //! tests for C++ behaviour fs.t.cpp does not cover.
    //!
    //! Adaptations, so the tests are hermetic and safe to run in parallel:
    //! - `/tmp` and the C++ relative `tmp` directory become directories in a
    //!   fresh temporary directory (`<base>/tmp` and `<base>/rel/tmp`).
    //! - `~` is the temporary home directory the test sets `HOME` to.
    //! - `Directory::cd` changes into the current directory (a no-op), so the
    //!   process CWD that other tests see never changes.
    //! - Tests that change `HOME`/`FOO` or the umask hold the crate-wide
    //!   [`crate::test_util::env_lock`] and
    //!   restore them afterwards.

    use super::*;
    use crate::test_util::{env_lock as lock, EnvGuard};

    /// Restores the umask when dropped.
    struct UmaskGuard(libc::mode_t);

    impl UmaskGuard {
        fn set(mask: libc::mode_t) -> Self {
            // SAFETY: umask cannot fail.
            UmaskGuard(unsafe { libc::umask(mask) })
        }
    }

    impl Drop for UmaskGuard {
        fn drop(&mut self) {
            // SAFETY: umask cannot fail.
            unsafe { libc::umask(self.0) };
        }
    }

    fn base() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_str().expect("UTF-8 temp path").to_string();
        (dir, path)
    }

    fn has(mode: u32, bit: libc::mode_t) -> bool {
        mode & bit as u32 != 0
    }

    fn is_root() -> bool {
        // SAFETY: geteuid cannot fail.
        unsafe { libc::geteuid() == 0 }
    }

    #[test]
    fn fs_t_cpp_path() {
        let _lock = lock();
        let _env = EnvGuard::save(&["HOME", "FOO"]);
        let (_dir, base) = base();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        std::env::set_var("HOME", &home);
        let slash_tmp = format!("{base}/tmp");
        std::fs::create_dir(&slash_tmp).unwrap();

        // Path ();
        let p0 = Path::default();
        assert_eq!(p0.data, "", "Path::Path");

        // Path (const Path&);
        let p1 = Path::new("/foo");
        assert_eq!(p1.data, "/foo", "Path::operator=");

        // Path (const std::string&);
        let p2 = Path::new("~");
        assert!(p2.data != "~", "~ expanded to {}", p2.data);

        let p3 = Path::new(&slash_tmp);
        assert!(p3.data == slash_tmp, "/tmp -> /tmp");

        // operator==
        assert!(p2 != p3, "p2 != p3");

        // Path& operator= (const Path&);
        let p3_copy = p3.clone();
        assert_eq!(p3.data, p3_copy.data, "Path::Path (Path&)");

        // operator (std::string) const;
        assert_eq!(String::from(&p3), slash_tmp, "Path::operator (std::string) const");

        // std::string name () const;
        let p4 = Path::new("/a/b/c/file.ext");
        assert_eq!(p4.name(), "file.ext", "/a/b/c/file.ext name is file.ext");

        // std::string parent () const;
        assert_eq!(p4.parent(), "/a/b/c", "/a/b/c/file.ext parent is /a/b/c");

        // std::string extension () const;
        assert_eq!(p4.extension(), "ext", "/a/b/c/file.ext extension is ext");

        // bool exists () const;
        let p4_fs = Path::new(&format!("{base}/a/b/c/file.ext"));
        assert!(p2.exists(), "~ exists");
        assert!(p3.exists(), "/tmp exists");
        assert!(!p4_fs.exists(), "/a/b/c/file.ext does not exist");

        // bool is_directory () const;
        assert!(p2.is_directory().unwrap(), "~ is_directory");
        assert!(p3.is_directory().unwrap(), "/tmp is_directory");

        // bool is_link () const;
        assert!(!p2.is_link().unwrap(), "~ !is_link");

        // bool readable () const;
        assert!(p2.readable().unwrap(), "~ readable");
        assert!(p3.readable().unwrap(), "/tmp readable");

        // bool writable () const;
        assert!(p2.writable().unwrap(), "~ writable");
        assert!(p3.writable().unwrap(), "/tmp writable");

        // bool executable () const;
        assert!(p2.executable().unwrap(), "~ executable");
        assert!(p3.executable().unwrap(), "/tmp executable");

        // static std::string expand (const std::string&);
        std::env::set_var("HOME", "/home/user");
        std::env::set_var("FOO", "hello/world");
        assert_eq!(Path::expand("~"), "/home/user", "Path::expand ~");
        assert_eq!(Path::expand("~/foo"), "/home/user/foo", "Path::expand ~/foo");
        // Relative paths are kept.
        assert_eq!(Path::expand("."), ".", "Path::expand . == .");
        assert_eq!(Path::expand("./"), "./", "Path::expand ./ == ./");
        assert_eq!(Path::expand(".a"), ".a", "Path::expand .a == .a");
        assert_eq!(
            Path::expand("./$FOO/bar"),
            "./hello/world/bar",
            "environment expanded expansion"
        );
        assert_eq!(
            Path::expand("~/$FOO/bar"),
            "/home/user/hello/world/bar",
            "tilde and environment expanded expansion"
        );

        // static std::vector <std::string> glob (const std::string&);
        let out = Path::glob(&slash_tmp);
        assert!(out.len() == 1, "/tmp -> 1 result");
        assert_eq!(out[0], slash_tmp, "/tmp -> /tmp");

        let out = Path::glob(&format!("{base}/t?p"));
        assert!(out.len() == 1, "/t?p -> 1 result");
        assert_eq!(out[0], slash_tmp, "/t?p -> /tmp");

        let out = Path::glob(&format!("{base}/[s-u]mp"));
        assert!(out.len() == 1, "/[s-u]mp -> 1 result");
        assert_eq!(out[0], slash_tmp, "/[s-u]mp -> /tmp");

        // bool is_absolute () const;
        assert!(!p0.is_absolute(), "'' !is_absolute");
        assert!(p1.is_absolute(), "/foo is_absolute");
        assert!(!Path::new("foo").is_absolute(), "foo !is_absolute");
        assert!(p2.is_absolute(), "~ is_absolute (after expansion)");
        assert!(p3.is_absolute(), "/tmp is_absolute");
        assert!(p4.is_absolute(), "/a/b/c/file.ext is_absolute");

        let p5 = Path::new("~/file.ext");
        assert!(p5.name() != "~/file.ext", "~/file.ext --> ! ~/file.ext");
    }

    #[test]
    fn fs_t_cpp_file_and_directory() {
        let _lock = lock();
        let (_dir, base) = base();
        // The C++ test's relative "tmp" directory.
        let tmp_path = format!("{base}/rel/tmp");
        let at = |name: &str| format!("{tmp_path}/{name}");

        let tmp = Directory::new(&tmp_path);
        tmp.create(Directory::DEFAULT_MODE);
        assert!(tmp.exists(), "tmp dir created.");

        File::write_file(&at("file.t.txt"), "This is a test\n");
        let f6 = File::new(&at("file.t.txt"));
        assert!(f6.size().unwrap() == 15, "File::size tmp/file.t.txt good");
        assert!(has(f6.mode().unwrap(), libc::S_IRUSR), "File::mode tmp/file.t.txt good");
        assert!(File::remove_file(&at("file.t.txt")), "File::remove tmp/file.t.txt good");

        // operator (std::string) const;
        assert_eq!(f6.to_string(), at("file.t.txt"), "File::operator (std::string) const");

        assert!(
            File::create_file(&at("file.t.create"), File::DEFAULT_MODE).unwrap(),
            "File::create tmp/file.t.create good"
        );
        assert!(File::remove_file(&at("file.t.create")), "File::remove tmp/file.t.create good");

        // basename (std::string) const;
        assert_eq!(f6.name(), "file.t.txt", "File::basename tmp/file.t.txt --> file.t.txt");

        // dirname (std::string) const;
        assert_eq!(f6.parent(), tmp_path, "File::dirname tmp/file.t.txt --> tmp");

        // bool rename (const std::string&);
        let mut f7 = File::new(&at("file.t.2.txt"));
        f7.append("something\n").unwrap();
        f7.close().unwrap();

        assert!(f7.rename(&at("file.t.3.txt")), "File::rename did not fail");
        assert_eq!(f7.data, at("file.t.3.txt"), "File::rename stored new name");
        assert!(f7.exists(), "File::rename new file exists");
        assert!(f7.remove(), "File::remove tmp/file.t.3.txt good");
        assert!(!f7.exists(), "File::remove new file no longer exists");

        // Test permissions.
        let mut f8 = File::new(&at("file.t.perm.txt"));
        f8.create(0o744).unwrap();
        assert!(f8.exists(), "File::create perm file exists");
        let m = f8.mode().unwrap();
        assert!(has(m, libc::S_IFREG), "File::mode tmp/file.t.perm.txt S_IFREG good");
        assert!(has(m, libc::S_IRUSR), "File::mode tmp/file.t.perm.txt r-------- good");
        assert!(has(m, libc::S_IWUSR), "File::mode tmp/file.t.perm.txt -w------- good");
        assert!(has(m, libc::S_IXUSR), "File::mode tmp/file.t.perm.txt --x------ good");
        assert!(has(m, libc::S_IRGRP), "File::mode tmp/file.t.perm.txt ---r----- good");
        assert!(!has(m, libc::S_IWGRP), "File::mode tmp/file.t.perm.txt ----w---- good");
        assert!(!has(m, libc::S_IXGRP), "File::mode tmp/file.t.perm.txt -----x--- good");
        assert!(has(m, libc::S_IROTH), "File::mode tmp/file.t.perm.txt ------r-- good");
        assert!(!has(m, libc::S_IWOTH), "File::mode tmp/file.t.perm.txt -------w- good");
        assert!(!has(m, libc::S_IXOTH), "File::mode tmp/file.t.perm.txt --------x good");
        f8.remove();
        assert!(!f8.exists(), "File::remove perm file no longer exists");

        File::write_file(&at("file.t.txt"), "This is a test\n");
        assert!(
            File::copy(&at("file.t.txt"), &at("file.t.copy.txt")),
            "File::copy returned true"
        );
        let f9 = File::new(&at("file.t.copy.txt"));
        assert!(f9.exists(), "File::copy created copy");
        assert!(f9.size().unwrap() == 15, "File::copy tmp/file.t.copy.txt good after copy");
        assert!(
            !File::copy(&at("does-not-exits.txt"), &at("should-not-exists.txt")),
            "File::copy returns false if not exists"
        );
        assert!(
            !File::new(&at("should-not-exists.txt")).exists(),
            "File::copy destination should not exist"
        );

        tmp.remove().unwrap();
        assert!(!tmp.exists(), "tmp dir removed.");
        tmp.create(Directory::DEFAULT_MODE);
        assert!(tmp.exists(), "tmp dir created.");

        // Directory (const File&);
        // Directory (const Path&);
        let d0 = Directory::from(Path::new(&tmp_path));
        let d1 = Directory::from(File::new(&tmp_path));
        let d2 = Directory::from(File::from(Path::new(&tmp_path)));
        assert_eq!(d0.data, d1.data, "Directory(std::string) == Directory (File&)");
        assert_eq!(d0.data, d2.data, "Directory(std::string) == Directory (File (Path &))");
        assert_eq!(d1.data, d2.data, "Directory(File&)) == Directory (File (Path &))");

        // Directory (const Directory&);
        let d3 = d2.clone();
        assert_eq!(d3.data, tmp_path, "Directory (Directory&)");

        // Directory (const std::string&);
        let d4 = Directory::new(&at("test_directory"));

        // Directory& operator= (const Directory&);
        let d5 = d4.clone();
        assert_eq!(d5.data, at("test_directory"), "Directory::operator=");

        // operator (std::string) const;
        assert_eq!(String::from(&d3), tmp_path, "Directory::operator (std::string) const");

        // virtual bool create ();
        assert!(d5.create(Directory::DEFAULT_MODE), "Directory::create tmp/test_directory");
        assert!(d5.exists(), "Directory::exists tmp/test_directory");

        let d6 = Directory::new(&format!("{}/dir", d5.data));
        assert!(d6.create(Directory::DEFAULT_MODE), "Directory::create tmp/test_directory/dir");

        File::create_file(&format!("{}/f0", d5.data), File::DEFAULT_MODE).unwrap();
        File::create_file(&format!("{}/f1", d6.data), File::DEFAULT_MODE).unwrap();

        // std::vector <std::string> list ();
        let mut files = d5.list().unwrap();
        files.sort();
        assert_eq!(files.len(), 2, "Directory::list 1 file");
        assert_eq!(files[0], at("test_directory/dir"), "file[0] is tmp/test_directory/dir");
        assert_eq!(files[1], at("test_directory/f0"), "file[1] is tmp/test_directory/f0");

        // std::vector <std::string> listRecursive ();
        let mut files = d5.list_recursive().unwrap();
        files.sort();
        assert_eq!(files.len(), 2, "Directory::list 1 file");
        assert_eq!(files[0], at("test_directory/dir/f1"), "file is tmp/test_directory/dir/f1");
        assert_eq!(files[1], at("test_directory/f0"), "file is tmp/test_directory/f0");

        // virtual bool remove ();
        assert!(File::remove_file(&format!("{}/f0", d5.data)), "File::remove tmp/test_directory/f0");
        assert!(
            File::remove_file(&format!("{}/f1", d6.data)),
            "File::remove tmp/test_directory/dir/f1"
        );

        assert!(d6.remove().unwrap(), "Directory::remove tmp/test_directory/dir");
        assert!(!d6.exists(), "Directory::exists tmp/test_directory/dir - no");

        assert!(d5.remove().unwrap(), "Directory::remove tmp/test_directory");
        assert!(!d5.exists(), "Directory::exists tmp/test_directory - no");

        // bool remove (const std::string&);
        let d7 = Directory::new(&at("to_be_removed"));
        assert!(d7.create(Directory::DEFAULT_MODE), "Directory::create tmp/to_be_removed");
        File::create_file(&format!("{}/f0", d7.data), File::DEFAULT_MODE).unwrap();
        let d8 = Directory::new(&format!("{}/another", d7.data));
        assert!(d8.create(Directory::DEFAULT_MODE), "Directory::create tmp/to_be_removed/another");
        File::create_file(&format!("{}/f1", d8.data), File::DEFAULT_MODE).unwrap();
        assert!(d7.remove().unwrap(), "Directory::remove tmp/to_be_removed");
        assert!(!d7.exists(), "Directory tmp/to_be_removed gone");

        // static std::string cwd ();
        let cwd = Directory::cwd();
        assert!(!cwd.is_empty(), "Directory::cwd returned a value");

        // bool parent (std::string&) const;
        let mut d9 = Directory::new("/one/two/three/four.txt");
        assert!(d9.up(), "parent /one/two/three/four.txt --> true");
        assert_eq!(d9.data, "/one/two/three", "parent /one/two/three/four.txt --> /one/two/three");
        assert!(d9.up(), "parent /one/two/three --> true");
        assert_eq!(d9.data, "/one/two", "parent /one/two/three --> /one/two");
        assert!(d9.up(), "parent /one/two --> true");
        assert_eq!(d9.data, "/one", "parent /one/two --> /one");
        assert!(d9.up(), "parent /one --> true");
        assert_eq!(d9.data, "/", "parent /one --> /");
        assert!(!d9.up(), "parent / --> false");

        // Test permissions.
        let _umask = UmaskGuard::set(0o022);
        let d10 = Directory::new(&at("dir.perm"));
        d10.create(0o750);
        assert!(d10.exists(), "Directory::create perm file exists");
        let m = d10.mode().unwrap();
        assert!(has(m, libc::S_IFDIR), "Directory::mode tmp/dir.perm S_IFDIR good");
        assert!(has(m, libc::S_IRUSR), "Directory::mode tmp/dir.perm r-------- good");
        assert!(has(m, libc::S_IWUSR), "Directory::mode tmp/dir.perm -w------- good");
        assert!(has(m, libc::S_IXUSR), "Directory::mode tmp/dir.perm --x------ good");
        assert!(has(m, libc::S_IRGRP), "Directory::mode tmp/dir.perm ---r----- good");
        assert!(!has(m, libc::S_IWGRP), "Directory::mode tmp/dir.perm ----w---- good");
        assert!(has(m, libc::S_IXGRP), "Directory::mode tmp/dir.perm -----x--- good");
        assert!(!has(m, libc::S_IROTH), "Directory::mode tmp/dir.perm ------r-- good");
        assert!(!has(m, libc::S_IWOTH), "Directory::mode tmp/dir.perm -------w- good");
        assert!(!has(m, libc::S_IXOTH), "Directory::mode tmp/dir.perm --------x good");
        d10.remove().unwrap();
        assert!(!d10.exists(), "Directory::remove temp/dir.perm file no longer exists");

        // Directory::cd (into the current directory, so the CWD is unchanged)
        let d11 = Directory::new(&cwd);
        assert!(d11.cd(), "Directory::cd /tmp good");

        tmp.remove().unwrap();
        assert!(!tmp.exists(), "tmp dir removed.");

        // File::removeBOM
        let line = "Should not be modified.";
        assert_eq!(
            File::remove_bom(line),
            line,
            "File::removeBOM 'Should not be modified' --> 'Should not be modified'"
        );

        let line = "no";
        assert_eq!(File::remove_bom(line), line, "File::removeBOM 'no' --> 'no'");

        let line = "";
        assert_eq!(File::remove_bom(line), line, "File::removeBOM '' --> ''");

        let line = "\u{FEFF}Foo";
        assert_eq!(File::remove_bom(line), "Foo", "File::removeBOM '<BOM>Foo' --> 'Foo'");
    }

    ////////////////////////////////////////////////////////////////////////////
    // Rust-only tests.

    #[test]
    fn path_strings_and_operators() {
        assert_eq!(Path::new("").name(), "");
        assert_eq!(Path::new("").parent(), "");
        assert_eq!(Path::new("file").parent(), ".");
        assert_eq!(Path::new("/file").parent(), "");
        assert_eq!(Path::new("dir/").name(), "");
        assert_eq!(Path::new("noext").extension(), "");
        assert_eq!(Path::new("a.b/c").extension(), "b/c", "last '.' anywhere, as in C++");

        let mut p = Path::new("a");
        p += "b";
        assert_eq!(p.data, "a/b");
        assert_eq!(p.original, "a");

        let x = Path {
            original: "x".into(),
            data: "same".into(),
        };
        let y = Path {
            original: "y".into(),
            data: "same".into(),
        };
        assert_eq!(x, y, "operator== compares _data only");
        assert_eq!(Path::from("q").data, "q");
        assert_eq!(Path::from(String::from("q")).to_string(), "q");
    }

    #[test]
    fn expand_edge_cases() {
        let _lock = lock();
        let _env = EnvGuard::save(&["HOME", "TW_FS_SET", "TW_FS_UNSET"]);
        std::env::set_var("TW_FS_SET", "v");
        std::env::remove_var("TW_FS_UNSET");

        assert_eq!(Path::expand(""), "");
        assert_eq!(Path::expand("a$"), "a$", "lone $ kept");
        assert_eq!(Path::expand("$/x"), "$/x", "$ before non-name kept");
        assert_eq!(Path::expand("$TW_FS_UNSET/x"), "/x", "unset variable is empty");
        assert_eq!(Path::expand("$TW_FS_SET.d"), "v.d");
        assert_eq!(Path::expand("a~b"), "a~b", "~ only at start");
        assert_eq!(Path::expand("~tw_no_such_user_x/y"), "/home/tw_no_such_user_x/y");
        assert_eq!(Path::expand("~tw_no_such_user_x"), "/home/tw_no_such_user_x");

        std::env::set_var("HOME", "");
        assert_eq!(Path::expand("~/x"), "/x", "empty HOME is used as is");

        std::env::remove_var("HOME");
        let passwd_home = home_of_current_user().map(|h| lossy(&h)).unwrap_or_default();
        assert_eq!(Path::expand("~"), passwd_home, "no HOME: passwd entry");

        // A NUL ends the path for C calls, as with c_str ().
        assert!(Path::new(".\0does-not-matter").exists());
    }

    #[test]
    fn glob_brace_and_no_match() {
        let (_dir, base) = base();
        File::write_file(&format!("{base}/a.txt"), "");
        File::write_file(&format!("{base}/b.txt"), "");
        let mut out = Path::glob(&format!("{base}/{{a,b}}.txt"));
        out.sort();
        assert_eq!(out, vec![format!("{base}/a.txt"), format!("{base}/b.txt")]);
        assert!(Path::glob(&format!("{base}/nothing*")).is_empty());
    }

    #[test]
    fn links_realpath_and_permissions() {
        let (_dir, base) = base();
        let target = format!("{base}/target");
        let link = format!("{base}/link");
        File::write_file(&target, "x");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(Path::new(&link).is_link().unwrap());
        assert!(!Path::new(&target).is_link().unwrap());
        assert!(!Path::new(&format!("{base}/missing")).is_link().unwrap());
        assert_eq!(Path::new(&link).realpath(), Path::new(&target).realpath());
        assert!(Path::new(&target).realpath().starts_with('/'));
        assert_eq!(Path::new("").realpath(), "");
        assert_eq!(Path::new(&format!("{base}/missing")).realpath(), "");
        assert!(!Path::new(&target).is_directory().unwrap());

        let missing = Path::new(&format!("{base}/missing"));
        assert!(!missing.readable().unwrap());
        assert!(!missing.writable().unwrap());
        assert!(!missing.executable().unwrap());
        assert!(!missing.is_directory().unwrap());

        // A dangling link does not "exist" (access follows links).
        std::os::unix::fs::symlink(format!("{base}/nowhere"), format!("{base}/dangling")).unwrap();
        assert!(!Path::new(&format!("{base}/dangling")).is_link().unwrap());

        let mut renamed = Path::new(&target);
        assert!(!renamed.rename(&target), "rename to the same name fails");
        assert!(renamed.rename(&format!("{base}/renamed")));
        assert_eq!(renamed.data, format!("{base}/renamed"));
        assert!(!renamed.rename(&format!("{base}/no/such/dir/x")));
        assert_eq!(renamed.data, format!("{base}/renamed"), "unchanged on failure");
    }

    #[test]
    fn open_permissions_error() {
        let (_dir, base) = base();
        let name = format!("{base}/locked");
        File::write_file(&name, "x");
        std::fs::set_permissions(&name, std::os::unix::fs::PermissionsExt::from_mode(0o400))
            .unwrap();
        let mut file = File::new(&name);
        if is_root() {
            // access () grants root everything.
            assert!(file.open().unwrap());
        } else {
            let err = file.open().unwrap_err();
            assert_eq!(err.to_string(), format!("Insufficient permissions for '{name}'."));
        }

        assert!(!File::default().open().unwrap(), "empty path does not open");
    }

    #[test]
    fn instance_read_semantics() {
        let (_dir, base) = base();
        let name = format!("{base}/r.txt");
        File::write_file(&name, "\u{FEFF}one\ntwo");
        let file = File::new(&name);
        assert_eq!(file.read().unwrap(), "one\ntwo\n", "BOM removed, last line terminated");
        assert_eq!(file.read_lines(), vec!["one", "two"]);

        File::write_file(&name, "a\n\nb\n");
        assert_eq!(file.read().unwrap(), "a\n\nb\n");
        assert_eq!(file.read_lines(), vec!["a", "", "b"]);

        File::write_file(&name, "\n");
        assert_eq!(file.read_lines(), vec![""]);
        File::write_file(&name, "x\u{FEFF}\n\u{FEFF}y\n");
        assert_eq!(file.read_lines(), vec!["x\u{FEFF}", "\u{FEFF}y"], "BOM only on line one");

        File::write_file(&name, "");
        assert_eq!(file.read().unwrap(), "");
        assert!(file.read_lines().is_empty());

        let missing = File::new(&format!("{base}/missing"));
        assert_eq!(
            missing.read().unwrap_err().to_string(),
            "stat error 2: No such file or directory"
        );
        assert!(missing.read_lines().is_empty());
        assert!(missing.size().is_err());
        assert!(missing.mode().is_err());
        assert!(missing.mtime().is_err());
        assert!(missing.ctime().is_err());
        assert!(missing.btime().is_err());

        assert!(file.mtime().unwrap() > 0);
        assert!(file.ctime().unwrap() > 0);
        assert!(file.btime().unwrap() > 0);
    }

    #[test]
    fn static_read_write_semantics() {
        let _lock = lock();
        let _env = EnvGuard::save(&["HOME"]);
        let (_dir, base) = base();
        std::env::set_var("HOME", &base);

        assert!(File::write_file_lines("~/l.txt", &["a", "b"], true));
        assert_eq!(File::read_file(&format!("{base}/l.txt")).unwrap(), "a\nb\n");
        assert!(File::write_file_lines("~/l.txt", &["a", "b"], false));
        assert_eq!(File::read_file(&format!("{base}/l.txt")).unwrap(), "ab\n");
        assert_eq!(File::read_file_lines(&format!("{base}/l.txt")).unwrap(), vec!["ab"]);
        assert!(File::write_file_lines::<&str>("~/l.txt", &[], true));
        assert_eq!(File::read_file(&format!("{base}/l.txt")).unwrap(), "");

        // Static reads do not expand.
        assert!(File::read_file("~/l.txt").is_none());
        assert!(File::read_file_lines("~/l.txt").is_none());
        // A directory opens but reads nothing, as with std::ifstream.
        assert_eq!(File::read_file(&base).unwrap(), "");

        assert!(!File::write_file(&format!("{base}/no/such/dir"), "x"));
        assert!(!File::write_file_lines(&format!("{base}/no/such/dir"), &["x"], true));
        assert!(!File::create_file(&format!("{base}/no/such/dir"), 0o600).unwrap());

        // Static create truncates and chmods exactly (no umask).
        File::write_file("~/c.txt", "old");
        assert!(File::create_file("~/c.txt", 0o604).unwrap());
        let created = File::new(&format!("{base}/c.txt"));
        assert_eq!(created.size().unwrap(), 0);
        assert_eq!(created.mode().unwrap() & 0o777, 0o604);

        // Instance create keeps the contents.
        File::write_file("~/k.txt", "keep");
        let mut kept = File::new("~/k.txt");
        assert!(kept.create(0o600).unwrap());
        assert_eq!(kept.size().unwrap(), 4);
        assert_eq!(kept.mode().unwrap() & 0o777, 0o600);
        assert!(!File::default().create(0o600).unwrap());

        assert!(File::remove_file("~/k.txt"));
        assert!(!File::remove_file("~/k.txt"));

        // Move: compares with the expanded destination, renames to it as given.
        let from = format!("{base}/m1");
        let to = format!("{base}/m2");
        File::write_file(&from, "m");
        assert!(!File::move_file(&from, &from));
        assert!(File::move_file(&from, &to));
        assert!(!Path::new(&from).exists());
        assert_eq!(File::read_file(&to).unwrap(), "m\n");
        assert!(!File::move_file(&from, &to), "source gone");
    }

    #[test]
    fn copy_quirks() {
        let (_dir, base) = base();
        let empty = format!("{base}/empty");
        File::write_file(&empty, "");
        assert!(!File::copy(&empty, &format!("{base}/empty.copy")), "C++ failbit on empty");
        assert!(Path::new(&format!("{base}/empty.copy")).exists(), "but the copy exists");

        let full = format!("{base}/full");
        File::write_file(&full, "abc");
        assert!(!File::copy(&full, &format!("{base}/no/such/dir")));
        assert!(File::copy(&full, &format!("{base}/full.copy")));
        assert_eq!(File::read_file(&format!("{base}/full.copy")).unwrap(), "abc\n");
    }

    #[test]
    fn handle_writes_lock_and_truncate() {
        let (_dir, base) = base();
        let name = format!("{base}/h.txt");
        let mut file = File::new(&name);
        assert!(!file.lock(), "not open");
        file.unlock();

        file.append("one\n").unwrap();
        file.append_lines(&["two\n", "three"]).unwrap();
        file.append("x\0ignored").unwrap();
        assert_eq!(File::read_file(&name).unwrap(), "one\ntwo\nthreex\n");
        assert!(file.lock());
        assert!(file.lock(), "lock again on the same descriptor");
        file.unlock();
        assert!(file.lock());

        // A copy has the path but no handle.
        let mut copy = file.clone();
        assert_eq!(copy, file);
        assert!(!copy.lock());
        file.close().unwrap();
        file.close().unwrap();
        assert!(!file.lock(), "closed");

        // write_raw writes at the position of a freshly opened handle (0).
        let mut raw = File::new(&name);
        raw.truncate().unwrap();
        assert_eq!(raw.size().unwrap(), 0);
        raw.write_raw("ab").unwrap();
        raw.write_raw("cd").unwrap();
        raw.close().unwrap();
        assert_eq!(File::read_file(&name).unwrap(), "abcd\n");

        let mut over = File::new(&name);
        over.write_raw("X").unwrap();
        drop(over);
        assert_eq!(File::read_file(&name).unwrap(), "Xbcd\n", "r+ keeps the rest");

        // assign: new path, same handle.
        let mut a = File::new(&name);
        assert!(a.open().unwrap());
        a.assign(&Path::new(&format!("{base}/other")));
        assert_eq!(a.data, format!("{base}/other"));
        assert!(a.lock(), "handle kept");

        // Writing without a path does nothing.
        let mut nothing = File::default();
        nothing.append("x").unwrap();
        nothing.write_raw("x").unwrap();
        nothing.truncate().unwrap();
    }

    #[test]
    fn directory_edge_cases() {
        let (_dir, base) = base();

        assert!(!Directory::new("").create(Directory::DEFAULT_MODE), "no endless recursion");
        assert!(Directory::new(&format!("{base}/x/y/z")).create(Directory::DEFAULT_MODE));
        assert!(Path::new(&format!("{base}/x/y/z")).is_directory().unwrap());
        assert!(!Directory::new(&format!("{base}/x")).create(Directory::DEFAULT_MODE), "exists");

        let mut rel = Directory::new("a/b");
        assert!(rel.up());
        assert_eq!(rel.data, "a");
        assert!(!rel.up());
        assert_eq!(rel.data, "a");

        assert!(Directory::new(&format!("{base}/missing")).list().unwrap().is_empty());
        assert!(!Directory::new(&format!("{base}/missing")).remove().unwrap());
        assert!(!Directory::new(&format!("{base}/missing")).cd());

        // Symbolic links are listed, not followed, and not removed through.
        let outside = format!("{base}/outside");
        Directory::new(&outside).create(Directory::DEFAULT_MODE);
        File::write_file(&format!("{outside}/keep"), "k");
        let tree = format!("{base}/tree");
        Directory::new(&format!("{tree}/sub")).create(Directory::DEFAULT_MODE);
        File::write_file(&format!("{tree}/sub/f"), "f");
        std::os::unix::fs::symlink(&outside, format!("{tree}/link")).unwrap();

        let mut all = Directory::new(&tree).list_recursive().unwrap();
        all.sort();
        assert_eq!(all, vec![format!("{tree}/link"), format!("{tree}/sub/f")]);
        let mut top = Directory::new(&tree).list().unwrap();
        top.sort();
        assert_eq!(top, vec![format!("{tree}/link"), format!("{tree}/sub")]);

        assert!(Directory::new(&tree).remove().unwrap());
        assert!(!Path::new(&tree).exists());
        assert!(Path::new(&format!("{outside}/keep")).exists());

        // Directory exposes the File API through Deref.
        let d = Directory::new(&outside);
        assert!(has(d.mode().unwrap(), libc::S_IFDIR));
        assert_eq!(d.to_string(), outside);
        assert_eq!(Directory::from(&File::new(&outside)), d);
        assert_eq!(Directory::from(&Path::new(&outside)), d);
    }

    #[test]
    fn cwd_matches_current_dir() {
        let cwd = Directory::cwd();
        let a = std::fs::canonicalize(&cwd).unwrap();
        let b = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        assert_eq!(a, b);
    }
}
