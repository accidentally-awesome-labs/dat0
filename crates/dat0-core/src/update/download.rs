use super::manifest::ArtifactEntry;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn download_verified(
    artifact: &ArtifactEntry,
    dest_dir: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    let name = artifact
        .url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("dat0-update.bin");
    let dest = dest_dir.join(name);
    // egress-seam: a bodyless GET for the artifact. Only the REQUEST counts —
    // the downloaded artifact is ingress, and counting it would turn the
    // status bar's egress figure into a download meter.
    crate::telemetry::egress::record_request(
        "GET",
        &artifact.url,
        crate::telemetry::egress::header_line_bytes("User-Agent", "dat0-updater"),
        0,
    );
    let resp = ureq::get(&artifact.url)
        .set("User-Agent", "dat0-updater")
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .context("GET artifact")?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&dest).context("create temp")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).context("read chunk")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).context("write chunk")?;
        done += n as u64;
        on_progress(done, artifact.size);
    }
    file.flush().context("flush")?;
    let got = format!("{:x}", hasher.finalize());
    if got != artifact.sha256.to_lowercase() {
        let _ = std::fs::remove_file(&dest);
        bail!("sha256 mismatch: expected {}, got {got}", artifact.sha256);
    }
    Ok(dest)
}
