use std::ffi::CStr;
use std::os::fd::RawFd;
use std::path::Path;

/// What is actually running in the foreground of a PTY. Read from the kernel (tcgetpgrp + sysctl on
/// macOS, `/proc` on Linux), never from terminal output, so the approval card can show the *real*
/// command, user and host in trusted window chrome — something no shipping terminal does and the
/// system Touch ID sheet omits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    /// Process ID (the foreground process group leader when taken from a PTY).
    pub pid: libc::pid_t,
    /// Absolute path of the executable image.
    pub executable_path: String,
    /// `argv`, including `argv[0]`.
    pub arguments: Vec<String>,
    /// Name of the effective user, or `"?"` when it could not be resolved.
    pub effective_user: String,
}

impl ProcessSnapshot {
    /// Option letters of `ssh` that take a separate value argument.
    const SSH_OPTIONS_WITH_VALUE: &'static [char] = &[
        'b', 'c', 'D', 'E', 'e', 'F', 'I', 'i', 'J', 'L', 'l', 'm', 'O', 'o', 'p', 'Q', 'R', 'S',
        'W', 'w',
    ];

    /// Last path component of the executable (e.g. `ssh`).
    pub fn executable_name(&self) -> &str {
        Path::new(&self.executable_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.executable_path)
    }

    /// The arguments, each shell-quoted when needed, joined by spaces.
    pub fn command_line(&self) -> String {
        self.arguments
            .iter()
            .map(|arg| Self::shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// True when the executable is `sudo`.
    pub fn is_sudo(&self) -> bool {
        self.executable_name() == "sudo"
    }

    /// True when the executable is `ssh`.
    pub fn is_ssh(&self) -> bool {
        self.executable_name() == "ssh"
    }

    /// For `ssh [opts] [user@]host [cmd]`: the destination argument. Best-effort (no full getopt parse).
    pub fn ssh_destination(&self) -> Option<&str> {
        if !self.is_ssh() {
            return None;
        }
        let args = &self.arguments;
        let mut i = 1;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--" {
                return args.get(i + 1).map(String::as_str);
            }
            let mut chars = arg.chars();
            if arg.starts_with('-') && arg.chars().count() >= 2 {
                chars.next();
                let takes_value = chars
                    .next()
                    .is_some_and(|flag| Self::SSH_OPTIONS_WITH_VALUE.contains(&flag));
                i += if takes_value && arg.chars().count() == 2 {
                    2
                } else {
                    1
                };
                continue;
            }
            return Some(arg);
        }
        None
    }

    /// Quotes `s` for a POSIX shell unless it consists only of alphanumerics and `@%+=:,./-_`.
    pub fn shell_quote(s: &str) -> String {
        if s.is_empty() {
            return "''".to_owned();
        }
        let is_safe = |c: char| c.is_alphanumeric() || "@%+=:,./-_".contains(c);
        if s.chars().all(is_safe) {
            return s.to_owned();
        }
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Reads process information straight from the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInspector;

impl ProcessInspector {
    /// The foreground process group leader of the terminal attached to `master_fd`.
    pub fn foreground(master_fd: RawFd) -> Option<ProcessSnapshot> {
        // SAFETY: tcgetpgrp only reads the descriptor; an invalid fd yields -1 rather than UB.
        let pgid = unsafe { libc::tcgetpgrp(master_fd) };
        if pgid <= 0 {
            return None;
        }
        Self::snapshot(pgid)
    }

    /// Snapshot of `pid`, or `None` when its executable path cannot be read.
    pub fn snapshot(pid: libc::pid_t) -> Option<ProcessSnapshot> {
        let path = platform::executable_path(pid)?;
        let arguments = platform::arguments(pid).unwrap_or_else(|| vec![path.clone()]);
        let effective_user = platform::effective_uid(pid)
            .and_then(user_name)
            .unwrap_or_else(|| "?".to_owned());
        Some(ProcessSnapshot {
            pid,
            executable_path: path,
            arguments,
            effective_user,
        })
    }
}

/// Resolves a user name with the reentrant `getpwuid_r`.
fn user_name(uid: libc::uid_t) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    loop {
        // SAFETY: passwd is plain old data; an all-zero value is valid and is filled in by getpwuid_r.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer refers to live, correctly sized storage owned by this frame.
        let rc =
            unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || result.is_null() || pwd.pw_name.is_null() {
            return None;
        }
        // SAFETY: on success pw_name points to a NUL-terminated string inside `buf`.
        let name = unsafe { CStr::from_ptr(pwd.pw_name) };
        return Some(name.to_string_lossy().into_owned());
    }
}

/// Splits `KERN_PROCARGS2` data: `[argc:int32][exec_path\0][padding \0…][argv0\0argv1\0…][env…]`.
#[cfg(any(target_os = "macos", test))]
fn parse_procargs2(data: &[u8]) -> Option<Vec<String>> {
    let header = data.get(..std::mem::size_of::<i32>())?;
    let argc = i32::from_ne_bytes(header.try_into().ok()?);
    let argc = usize::try_from(argc).unwrap_or(0);
    let size = data.len();
    let mut i = std::mem::size_of::<i32>();
    while i < size && data[i] != 0 {
        i += 1; // skip exec path
    }
    while i < size && data[i] == 0 {
        i += 1; // skip padding
    }
    let mut args = Vec::new();
    let mut start = i;
    while i < size && args.len() < argc {
        if data[i] == 0 {
            args.push(String::from_utf8_lossy(&data[start..i]).into_owned());
            start = i + 1;
        }
        i += 1;
    }
    Some(args)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::parse_procargs2;

    /// PROC_PIDPATHINFO_MAXSIZE.
    const PATH_MAX_SIZE: usize = 4096;
    /// `sizeof(struct kinfo_proc)` on 64-bit macOS (libc does not bind this struct).
    const KINFO_PROC_SIZE: usize = 648;
    /// Offset of `kp_eproc.e_ucred.cr_uid`: extern_proc (296) + e_paddr (8) + e_sess (8)
    /// + _pcred (104) + _ucred.cr_ref (4).
    const KINFO_PROC_CR_UID_OFFSET: usize = 420;

    pub(super) fn executable_path(pid: libc::pid_t) -> Option<String> {
        let mut buf = vec![0u8; PATH_MAX_SIZE];
        // SAFETY: `buf` is writable for `buf.len()` bytes; the length fits in u32.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        let n = usize::try_from(n).ok().filter(|&n| n > 0)?;
        Some(String::from_utf8_lossy(&buf[..n.min(buf.len())]).into_owned())
    }

    pub(super) fn arguments(pid: libc::pid_t) -> Option<Vec<String>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size: libc::size_t = 0;
        // SAFETY: a null output buffer asks sysctl only for the required size.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size <= std::mem::size_of::<i32>() {
            return None;
        }
        let mut data = vec![0u8; size];
        // SAFETY: `data` is writable for `size` bytes, and sysctl updates `size` to what it wrote.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                data.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        data.truncate(size);
        parse_procargs2(&data)
    }

    pub(super) fn effective_uid(pid: libc::pid_t) -> Option<libc::uid_t> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        let mut info = [0u8; KINFO_PROC_SIZE];
        let mut size: libc::size_t = KINFO_PROC_SIZE;
        // SAFETY: `info` is writable for `size` bytes, and sysctl updates `size` to what it wrote.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                info.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size < KINFO_PROC_CR_UID_OFFSET + 4 {
            return None;
        }
        let bytes = info.get(KINFO_PROC_CR_UID_OFFSET..KINFO_PROC_CR_UID_OFFSET + 4)?;
        Some(libc::uid_t::from_ne_bytes(bytes.try_into().ok()?))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;

    pub(super) fn executable_path(pid: libc::pid_t) -> Option<String> {
        let path = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        Some(path.to_string_lossy().into_owned())
    }

    pub(super) fn arguments(pid: libc::pid_t) -> Option<Vec<String>> {
        let data = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        Some(super::parse_cmdline(&data))
    }

    pub(super) fn effective_uid(pid: libc::pid_t) -> Option<libc::uid_t> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        super::parse_status_euid(&status)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    pub(super) fn executable_path(_pid: libc::pid_t) -> Option<String> {
        None
    }

    pub(super) fn arguments(_pid: libc::pid_t) -> Option<Vec<String>> {
        None
    }

    pub(super) fn effective_uid(_pid: libc::pid_t) -> Option<libc::uid_t> {
        None
    }
}

/// Splits `/proc/<pid>/cmdline` (NUL-separated, NUL-terminated) into arguments.
#[cfg(any(target_os = "linux", test))]
fn parse_cmdline(data: &[u8]) -> Vec<String> {
    let data = data.strip_suffix(&[0]).unwrap_or(data);
    if data.is_empty() {
        return Vec::new();
    }
    data.split(|&b| b == 0)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

/// The effective UID: the second field of the `Uid:` line in `/proc/<pid>/status`.
#[cfg(any(target_os = "linux", test))]
fn parse_status_euid(status: &str) -> Option<libc::uid_t> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1))
        .and_then(|euid| euid.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(path: &str, args: &[&str]) -> ProcessSnapshot {
        ProcessSnapshot {
            pid: 1,
            executable_path: path.to_owned(),
            arguments: args.iter().map(|&a| a.to_owned()).collect(),
            effective_user: "me".to_owned(),
        }
    }

    #[test]
    fn executable_name_is_last_path_component() {
        let s = snapshot("/usr/bin/sudo", &["sudo", "ls"]);
        assert_eq!(s.executable_name(), "sudo");
        assert!(s.is_sudo());
        assert!(!s.is_ssh());
    }

    #[test]
    fn ssh_destination_skips_options_that_take_values() {
        let s = snapshot(
            "/usr/bin/ssh",
            &[
                "ssh",
                "-p",
                "2222",
                "-v",
                "-i",
                "key",
                "-oBatchMode=yes",
                "deploy@prod-db-1",
                "uptime",
            ],
        );
        assert_eq!(s.ssh_destination(), Some("deploy@prod-db-1"));
    }

    #[test]
    fn ssh_destination_after_double_dash() {
        let s = snapshot("/usr/bin/ssh", &["ssh", "-4", "--", "host"]);
        assert_eq!(s.ssh_destination(), Some("host"));
        let trailing = snapshot("/usr/bin/ssh", &["ssh", "--"]);
        assert_eq!(trailing.ssh_destination(), None);
    }

    #[test]
    fn ssh_destination_is_none_for_other_commands_or_missing_host() {
        assert_eq!(
            snapshot("/usr/bin/sudo", &["sudo", "host"]).ssh_destination(),
            None
        );
        assert_eq!(
            snapshot("/usr/bin/ssh", &["ssh", "-l", "user"]).ssh_destination(),
            None
        );
    }

    #[test]
    fn shell_quote_leaves_safe_strings_alone_and_quotes_the_rest() {
        assert_eq!(ProcessSnapshot::shell_quote(""), "''");
        assert_eq!(
            ProcessSnapshot::shell_quote("user@host:/a,b.c-d_e%f+g=h"),
            "user@host:/a,b.c-d_e%f+g=h"
        );
        assert_eq!(ProcessSnapshot::shell_quote("two words"), "'two words'");
        assert_eq!(ProcessSnapshot::shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn command_line_joins_quoted_arguments() {
        let s = snapshot("/bin/echo", &["echo", "hello world", "$HOME"]);
        assert_eq!(s.command_line(), "echo 'hello world' '$HOME'");
    }

    #[test]
    fn parses_procargs2_layout() {
        let mut data = 2i32.to_ne_bytes().to_vec();
        data.extend_from_slice(b"/usr/bin/ssh\0\0\0\0ssh\0host\0PATH=/bin\0");
        assert_eq!(
            parse_procargs2(&data),
            Some(vec!["ssh".to_owned(), "host".to_owned()])
        );
        assert_eq!(parse_procargs2(&[1, 0]), None);
    }

    #[test]
    fn parses_proc_cmdline_and_status() {
        assert_eq!(
            parse_cmdline(b"ssh\0-p\x002222\0host\0"),
            vec!["ssh", "-p", "2222", "host"]
        );
        assert!(parse_cmdline(b"").is_empty());
        let status = "Name:\tsudo\nUid:\t501\t0\t0\t0\nGid:\t20\t20\t20\t20\n";
        assert_eq!(parse_status_euid(status), Some(0));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn snapshots_the_current_process() {
        let pid = libc::pid_t::try_from(std::process::id()).unwrap();
        let snap = ProcessInspector::snapshot(pid).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            snap.executable_name(),
            exe.file_name().unwrap().to_str().unwrap()
        );
        assert!(!snap.arguments.is_empty());
        assert_ne!(snap.effective_user, "?");
    }

    #[test]
    fn foreground_of_invalid_fd_is_none() {
        assert_eq!(ProcessInspector::foreground(-1), None);
    }
}
