//! Self-update — **M7**. GitHub Releases + SHA-256 + Ed25519 signature; release builds refuse an
//! unsigned package. Point `GITHUB_REPO` at **our** repo. Gated by `TrackingConfig.auto_update`
//! (added in the M3a contract PR, §6).

/// The running agent version — folds into the heartbeat and gates update checks.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// TODO(M7): verify.rs (SHA-256 + Ed25519) · install.rs (staged apply + restart).
