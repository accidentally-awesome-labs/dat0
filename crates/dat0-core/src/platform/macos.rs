use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    // Test / portable-install relocation seam: a non-empty `DAT0_CONFIG_DIR`
    // overrides the default location verbatim. When unset/empty the body below
    // is reached and behaves byte-identically to before this seam existed.
    if let Some(p) = std::env::var_os("DAT0_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    Ok(dirs::home_dir()
        .context("no home")?
        .join("Library/Application Support/dat0"))
}

pub fn data_dir() -> Result<PathBuf> {
    config_dir()
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("no home")?
        .join("Library/Caches/dat0"))
}

/// Open `url` in the user's default browser via the macOS `open` shell tool.
pub fn open_url(url: &str) -> Result<()> {
    let status = std::process::Command::new("open").arg(url).status()?;
    anyhow::ensure!(status.success(), "open_url failed for {url}");
    Ok(())
}

/// Resident set size of this process, in bytes. `None` on any failure.
///
/// MX1's perf HUD and the `idle_rss` scenario both need a memory number that
/// costs nothing to sample, so this is a `proc_pidinfo` call rather than a
/// dependency. It is best-effort by contract: a caller renders an em-dash on
/// `None` and never treats the absence as an error.
pub fn rss_bytes() -> Option<u64> {
    let mut ti = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `ti` is a zeroed, correctly-sized, correctly-aligned
    // `proc_taskinfo` owned by this frame, and `size` is its exact byte length,
    // so the kernel cannot write out of bounds. `proc_pidinfo` only writes; it
    // retains no pointer past the call.
    let rc = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            ti.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    // The call returns the number of bytes written; a short write means the
    // struct was not populated and the value would be the zeroes we supplied.
    if rc != size {
        return None;
    }
    // SAFETY: the kernel reported a full-size write into `ti`, so every field
    // is initialized.
    Some(unsafe { ti.assume_init() }.pti_resident_size)
}
