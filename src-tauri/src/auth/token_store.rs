//! Chunked token storage in the OS keyring (Keychain / Credential Manager / Secret Service). The
//! Windows Credential Manager caps a credential blob at ~2560 bytes; a Cognito refresh token can
//! exceed that, so the blob is base64'd and split into `<key>.0..N` entries plus a `<key>.count`
//! (BUILD-PLAN M1). The split/join logic is pure + tested; the keyring I/O is a thin wrapper
//! (untested — a real secret service isn't available headless; risk #10).
//!
//! Named `token_store` (not `keyring`) to avoid shadowing the `keyring` crate.

use base64::Engine;
use keyring::Entry;

const SERVICE: &str = "com.workpulse.agent";
/// Base64 chars per keyring entry — comfortably under the CredMan blob limit.
const CHUNK: usize = 2000;

fn b64(blob: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(blob)
}

/// Split a blob into keyring-sized base64 chunks (pure — base64 is ASCII so byte-chunking is safe).
fn split(blob: &[u8]) -> Vec<String> {
    b64(blob)
        .into_bytes()
        .chunks(CHUNK)
        .map(|c| String::from_utf8(c.to_vec()).expect("base64 is ascii"))
        .collect()
}

/// Rejoin + decode chunks back into the original blob.
fn join(chunks: &[String]) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(chunks.concat())
        .ok()
}

fn entry(key: &str) -> keyring::Result<Entry> {
    Entry::new(SERVICE, key)
}

/// Store a blob under `key` (base64 + chunked). Clears any previous (possibly longer) set first.
pub fn store(key: &str, blob: &[u8]) -> keyring::Result<()> {
    let _ = clear(key);
    let chunks = split(blob);
    for (i, c) in chunks.iter().enumerate() {
        entry(&format!("{key}.{i}"))?.set_password(c)?;
    }
    entry(&format!("{key}.count"))?.set_password(&chunks.len().to_string())?;
    Ok(())
}

/// Load a previously stored blob, or `None` if absent/corrupt.
pub fn load(key: &str) -> Option<Vec<u8>> {
    let count: usize = entry(&format!("{key}.count"))
        .ok()?
        .get_password()
        .ok()?
        .parse()
        .ok()?;
    let mut chunks = Vec::with_capacity(count);
    for i in 0..count {
        chunks.push(entry(&format!("{key}.{i}")).ok()?.get_password().ok()?);
    }
    join(&chunks)
}

/// Remove every entry for `key`.
pub fn clear(key: &str) -> keyring::Result<()> {
    if let Ok(count_entry) = entry(&format!("{key}.count")) {
        if let Ok(count) = count_entry
            .get_password()
            .and_then(|s| s.parse::<usize>().map_err(|_| keyring::Error::NoEntry))
        {
            for i in 0..count {
                if let Ok(e) = entry(&format!("{key}.{i}")) {
                    let _ = e.delete_credential();
                }
            }
        }
        let _ = count_entry.delete_credential();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_round_trip_over_the_limit() {
        // A blob whose base64 exceeds one chunk → must split into >1 and rejoin exactly.
        let blob: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let chunks = split(&blob);
        assert!(chunks.len() > 1, "should split past the chunk limit");
        assert!(chunks.iter().all(|c| c.len() <= CHUNK));
        assert_eq!(join(&chunks).unwrap(), blob);
    }

    #[test]
    fn small_blob_single_chunk() {
        let blob = b"refresh-token-value";
        let chunks = split(blob);
        assert_eq!(chunks.len(), 1);
        assert_eq!(join(&chunks).unwrap(), blob);
    }
}
