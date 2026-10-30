//! Where a game goes on a drive, and free space. The drives themselves are rare-library's
//! (`storage::drives`, the list Settings > Storage shows).

use crate::Store;
use std::path::{Path, PathBuf};

/// `<root>/Games/Epic` or `<root>/Games/GOG` (root: the home folder for the internal disk, the
/// mount point for an SD card or USB drive).
pub fn games_dir(root: &Path, store: Store) -> PathBuf {
    root.join("Games").join(store.folder())
}

/// (free, total) bytes of the filesystem `p` is on.
pub fn space(p: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return None;
        }
        let f = s.f_frsize as u64;
        Some((s.f_bavail as u64 * f, s.f_blocks as u64 * f))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        None
    }
}
