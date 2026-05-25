//! `sample_data::fetch_remote` SHA256-verified download + atomic cache (P3b T8).
//!
//! Each test stands up a mockito server bound to an ephemeral port, points
//! `fetch_remote` at it, and asserts the cache contract:
//!
//! - Happy path: file appears at `<state_root>/samples/<dest>` with matching sha.
//! - Checksum mismatch: error contains "checksum"; no cache file written.
//! - Cache hit: pre-populated dest causes the URL to NOT be hit
//!   (mockito `.expect(0)`).
//! - HTTP 404: error mentions the status code.
//!
//! All tests are `#[tokio::test]` because `fetch_remote` is async. The
//! workspace tokio dependency already enables `macros` + `rt-multi-thread`
//! (see workspace Cargo.toml `[workspace.dependencies] tokio` features).

use sha2::{Digest, Sha256};
use tempfile::tempdir;

use dat0_app::sample_data::fetch_remote;

const TEST_PAYLOAD: &[u8] = b"hello, parquet payload for sample_data_fetch test";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[tokio::test]
async fn fetches_and_caches_with_matching_sha() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/nyc_taxi.parquet")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(TEST_PAYLOAD)
        .create_async()
        .await;

    let dir = tempdir().unwrap();
    let url = format!("{}/nyc_taxi.parquet", server.url());
    let expected = sha256_hex(TEST_PAYLOAD);

    let path = fetch_remote(&url, &expected, dir.path(), "nyc_taxi.parquet")
        .await
        .expect("fetch_remote should succeed on matching sha");

    assert!(path.exists(), "destination file should exist at {path:?}");
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(on_disk, TEST_PAYLOAD, "cached file matches payload");
    assert_eq!(
        sha256_hex(&on_disk),
        expected,
        "on-disk sha matches expected"
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn sha_mismatch_fails_without_caching() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/nyc_taxi.parquet")
        .with_status(200)
        .with_body(TEST_PAYLOAD)
        .create_async()
        .await;

    let dir = tempdir().unwrap();
    let url = format!("{}/nyc_taxi.parquet", server.url());
    let bogus_sha = "0".repeat(64); // 32 zero bytes in hex

    let err = fetch_remote(&url, &bogus_sha, dir.path(), "nyc_taxi.parquet")
        .await
        .expect_err("fetch_remote should fail on sha mismatch");

    let msg = format!("{err:#}");
    assert!(
        msg.to_lowercase().contains("checksum"),
        "error mentions checksum; got: {msg}"
    );

    let dest = dir.path().join("samples").join("nyc_taxi.parquet");
    assert!(
        !dest.exists(),
        "cache file must NOT exist on checksum failure (found {dest:?})"
    );
    let part = dir.path().join("samples").join("nyc_taxi.parquet.part");
    assert!(
        !part.exists(),
        "temp .part file must not linger after checksum failure (found {part:?})"
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn cached_file_short_circuits_fetch() {
    let mut server = mockito::Server::new_async().await;
    // .expect(0) — fetch_remote must NOT touch the network when the cache
    // file already exists.
    let mock = server
        .mock("GET", "/nyc_taxi.parquet")
        .expect(0)
        .with_status(200)
        .with_body(TEST_PAYLOAD)
        .create_async()
        .await;

    let dir = tempdir().unwrap();
    let samples = dir.path().join("samples");
    std::fs::create_dir_all(&samples).unwrap();
    let pre = samples.join("nyc_taxi.parquet");
    std::fs::write(&pre, b"prepopulated-cache-content").unwrap();

    let url = format!("{}/nyc_taxi.parquet", server.url());
    // Pass a deliberately wrong sha — cache-hit short-circuit must skip
    // verification entirely (the file is trusted-from-disk).
    let bogus_sha = "0".repeat(64);

    let path = fetch_remote(&url, &bogus_sha, dir.path(), "nyc_taxi.parquet")
        .await
        .expect("cache-hit should short-circuit without verifying");

    assert_eq!(path, pre, "returned path equals pre-populated cache file");
    assert!(path.exists());
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(
        on_disk, b"prepopulated-cache-content",
        "cache content untouched by fetch_remote"
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn http_404_returns_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/nyc_taxi.parquet")
        .with_status(404)
        .with_body("not found")
        .create_async()
        .await;

    let dir = tempdir().unwrap();
    let url = format!("{}/nyc_taxi.parquet", server.url());
    // Any sha — we should never get to the verify step.
    let any_sha = "0".repeat(64);

    let err = fetch_remote(&url, &any_sha, dir.path(), "nyc_taxi.parquet")
        .await
        .expect_err("fetch_remote should fail on 404");

    let msg = format!("{err:#}");
    let lower = msg.to_lowercase();
    assert!(
        lower.contains("404") || lower.contains("status"),
        "error mentions 404 or status; got: {msg}"
    );

    let dest = dir.path().join("samples").join("nyc_taxi.parquet");
    assert!(!dest.exists(), "no cache file on 404 (found {dest:?})");

    mock.assert_async().await;
}
