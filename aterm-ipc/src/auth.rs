use std::os::unix::net::UnixStream;

/// Verify connecting peer has the same UID as this process.
pub fn verify_peer(stream: &UnixStream) -> bool {
    #[cfg(target_os = "macos")]
    {
        verify_peer_macos(stream)
    }
    #[cfg(target_os = "linux")]
    {
        verify_peer_linux(stream)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = stream;
        true // Fallback: allow on unsupported platforms
    }
}

#[cfg(target_os = "macos")]
fn verify_peer_macos(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    unsafe {
        libc::getpeereid(fd, &mut uid, &mut gid) == 0 && uid == libc::getuid()
    }
}

#[cfg(target_os = "linux")]
fn verify_peer_linux(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        ) == 0
            && cred.uid == libc::getuid()
    }
}
