use std::os::fd::RawFd;
use std::path::Path;

use glasshouse_security::{ProcessInspector, ProcessSnapshot};

/// Who is on the other end of the agent socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Effective UID of the peer.
    pub uid: libc::uid_t,
    /// PID of the peer, when the kernel reports it.
    pub pid: Option<libc::pid_t>,
    /// Path from the code-signing identity bound to the audit token — not from the PID, which can be
    /// reused. Always `None` off macOS.
    pub code_path: Option<String>,
    /// True when the peer's code signature is valid. Always false off macOS.
    pub code_is_validly_signed: bool,
    /// argv of the peer (normally `ssh …`), for showing the destination in the Touch ID prompt.
    pub process: Option<ProcessSnapshot>,
}

impl PeerIdentity {
    /// Executable name of the peer, falling back to the code path's last component.
    pub fn display_name(&self) -> String {
        if let Some(process) = &self.process {
            return process.executable_name().to_owned();
        }
        if let Some(path) = &self.code_path {
            return Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
        }
        "unknown process".to_owned()
    }

    /// Identifies the peer of the connected Unix socket `fd`, or `None` if its credentials are unavailable.
    pub fn identify(fd: RawFd) -> Option<Self> {
        platform::identify(fd)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub(super) fn identify(fd: RawFd) -> Option<PeerIdentity> {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        // SAFETY: both out-pointers refer to live locals.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return None;
        }

        let mut pid: libc::pid_t = 0;
        let mut pid_len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: `pid` is writable for `pid_len` bytes.
        let have_pid = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &mut pid_len,
            )
        } == 0;

        // audit_token_t is eight 32-bit words.
        let mut token = [0u32; 8];
        let mut token_len = std::mem::size_of_val(&token) as libc::socklen_t;
        // SAFETY: `token` is writable for `token_len` bytes.
        let have_token = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERTOKEN,
                token.as_mut_ptr().cast(),
                &mut token_len,
            )
        } == 0;
        let (code_path, valid) = if have_token {
            let bytes: Vec<u8> = token.iter().flat_map(|word| word.to_ne_bytes()).collect();
            code_identity::code_identity(&bytes)
        } else {
            (None, false)
        };

        Some(PeerIdentity {
            uid,
            pid: have_pid.then_some(pid),
            code_path,
            code_is_validly_signed: valid,
            process: if have_pid {
                ProcessInspector::snapshot(pid)
            } else {
                None
            },
        })
    }

    /// Minimal Security.framework / CoreFoundation bindings for guest code lookup by audit token.
    mod code_identity {
        use std::ffi::{CStr, c_void};

        type CFTypeRef = *const c_void;
        type OSStatus = i32;

        #[repr(C)]
        struct Opaque {
            _private: [u8; 0],
        }

        const ERR_SEC_SUCCESS: OSStatus = 0;
        const PATH_BUFFER_LEN: usize = 4096;

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            static kCFTypeDictionaryKeyCallBacks: Opaque;
            static kCFTypeDictionaryValueCallBacks: Opaque;
            fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
            fn CFDictionaryCreate(
                allocator: CFTypeRef,
                keys: *const CFTypeRef,
                values: *const CFTypeRef,
                count: isize,
                key_callbacks: *const Opaque,
                value_callbacks: *const Opaque,
            ) -> CFTypeRef;
            fn CFURLGetFileSystemRepresentation(
                url: CFTypeRef,
                resolve: u8,
                buffer: *mut u8,
                len: isize,
            ) -> u8;
            fn CFRelease(cf: CFTypeRef);
        }

        #[link(name = "Security", kind = "framework")]
        unsafe extern "C" {
            static kSecGuestAttributeAudit: CFTypeRef;
            fn SecCodeCopyGuestWithAttributes(
                host: CFTypeRef,
                attributes: CFTypeRef,
                flags: u32,
                guest: *mut CFTypeRef,
            ) -> OSStatus;
            fn SecCodeCheckValidity(
                code: CFTypeRef,
                flags: u32,
                requirement: CFTypeRef,
            ) -> OSStatus;
            fn SecCodeCopyStaticCode(
                code: CFTypeRef,
                flags: u32,
                static_code: *mut CFTypeRef,
            ) -> OSStatus;
            fn SecCodeCopyPath(
                static_code: CFTypeRef,
                flags: u32,
                path: *mut CFTypeRef,
            ) -> OSStatus;
        }

        /// Releases a non-null CF object when dropped.
        struct Owned(CFTypeRef);

        impl Drop for Owned {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    // SAFETY: we own one reference (from a Create/Copy call) to this object.
                    unsafe { CFRelease(self.0) };
                }
            }
        }

        /// (code path, signature valid) for the process identified by `audit_token`.
        pub(super) fn code_identity(audit_token: &[u8]) -> (Option<String>, bool) {
            let Ok(len) = isize::try_from(audit_token.len()) else {
                return (None, false);
            };
            // SAFETY: `audit_token` is readable for `len` bytes; CFDataCreate copies them.
            let data = Owned(unsafe { CFDataCreate(std::ptr::null(), audit_token.as_ptr(), len) });
            if data.0.is_null() {
                return (None, false);
            }
            // SAFETY: the key is a valid CFString constant and the value a live CFData; the CFType
            // callbacks retain both for the dictionary's lifetime.
            let attrs = Owned(unsafe {
                let key = kSecGuestAttributeAudit;
                CFDictionaryCreate(
                    std::ptr::null(),
                    &key,
                    &data.0,
                    1,
                    &raw const kCFTypeDictionaryKeyCallBacks,
                    &raw const kCFTypeDictionaryValueCallBacks,
                )
            });
            if attrs.0.is_null() {
                return (None, false);
            }

            let mut guest: CFTypeRef = std::ptr::null();
            // SAFETY: `attrs` is a valid dictionary; `guest` receives an owned SecCode on success.
            let status =
                unsafe { SecCodeCopyGuestWithAttributes(std::ptr::null(), attrs.0, 0, &mut guest) };
            let guest = Owned(guest);
            if status != ERR_SEC_SUCCESS || guest.0.is_null() {
                return (None, false);
            }
            // SAFETY: `guest` is a valid SecCode; a null requirement checks the code's own designated one.
            let valid =
                unsafe { SecCodeCheckValidity(guest.0, 0, std::ptr::null()) } == ERR_SEC_SUCCESS;

            let mut static_code: CFTypeRef = std::ptr::null();
            // SAFETY: `guest` is a valid SecCode; `static_code` receives an owned SecStaticCode.
            let status = unsafe { SecCodeCopyStaticCode(guest.0, 0, &mut static_code) };
            let static_code = Owned(static_code);
            if status != ERR_SEC_SUCCESS || static_code.0.is_null() {
                return (None, valid);
            }
            let mut url: CFTypeRef = std::ptr::null();
            // SAFETY: `static_code` is a valid SecStaticCode; `url` receives an owned CFURL.
            let status = unsafe { SecCodeCopyPath(static_code.0, 0, &mut url) };
            let url = Owned(url);
            if status != ERR_SEC_SUCCESS || url.0.is_null() {
                return (None, valid);
            }
            let mut buf = vec![0u8; PATH_BUFFER_LEN];
            // SAFETY: `url` is a valid CFURL and `buf` is writable for its full length.
            let ok = unsafe {
                CFURLGetFileSystemRepresentation(url.0, 1, buf.as_mut_ptr(), buf.len() as isize)
            };
            if ok == 0 {
                return (None, valid);
            }
            let path = CStr::from_bytes_until_nul(&buf)
                .ok()
                .map(|path| path.to_string_lossy().into_owned());
            (path, valid)
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    pub(super) fn identify(fd: RawFd) -> Option<PeerIdentity> {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` is writable for `len` bytes.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast(),
                &mut len,
            )
        };
        if rc != 0 {
            return None;
        }
        let pid = (cred.pid > 0).then_some(cred.pid);
        Some(PeerIdentity {
            uid: cred.uid,
            pid,
            code_path: None,
            code_is_validly_signed: false,
            process: pid.and_then(ProcessInspector::snapshot),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;

    pub(super) fn identify(_fd: RawFd) -> Option<PeerIdentity> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_prefers_process_then_code_path() {
        let mut peer = PeerIdentity {
            uid: 0,
            pid: None,
            code_path: Some("/usr/bin/ssh".to_owned()),
            code_is_validly_signed: true,
            process: None,
        };
        assert_eq!(peer.display_name(), "ssh");
        peer.code_path = None;
        assert_eq!(peer.display_name(), "unknown process");
    }

    #[test]
    fn identifies_socket_pair_peer_as_this_process() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = PeerIdentity::identify(std::os::fd::AsRawFd::as_raw_fd(&a)).unwrap();
        // SAFETY: getuid has no preconditions.
        assert_eq!(peer.uid, unsafe { libc::getuid() });
        let pid = libc::pid_t::try_from(std::process::id()).unwrap();
        assert_eq!(peer.pid, Some(pid));
        assert_eq!(peer.process.unwrap().pid, pid);
    }
}
