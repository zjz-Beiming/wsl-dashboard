// SPDX-FileCopyrightText: Copyright (c) 2026 owu <wqh@live.com>
// SPDX-License-Identifier: GPL-3.0-only

//! WSL version compatibility policy.
//!
//! ## Why this module exists
//!
//! The compatibility banner is driven by a *remote* allow-list:
//! `wsl_support.since_version` / `wsl_support.until_version` served by the
//! bootstrap API. That list can only be refreshed server-side, so it always
//! lags behind the WSL release train. A user who installs a newer WSL (or
//! simply moves to the next stable line) therefore gets a scary
//! "unsupported version / possible data loss" dialog even though every
//! command the app issues still behaves exactly the same.
//!
//! Historically the upper bound had to be conservative because the app guessed
//! capability from the version number. It no longer does: every VHD-touching
//! operation is now gated by a *runtime capability probe* (see
//! [`crate::wsl::ops::config::WslVersionMeta`]), which inspects `wsl --help`
//! instead of trusting a version string. A future WSL that renames or removes
//! a switch is detected at probe time and the app falls back gracefully.
//!
//! ## Policy
//!
//! * `current < min`  -> [`VersionVerdict::TooOld`]      -> keep the warning.
//! * `min <= current <= max` -> [`VersionVerdict::Verified`] -> no dialog.
//! * `current > max`  -> [`VersionVerdict::NewerThanVerified`] -> no dialog,
//!   only a log line, because capability probing covers the risk.
//!
//! `min` is the stricter of the built-in floor and the server-provided
//! `since_version`; `max` is the newer of the built-in ceiling and the
//! server-provided `until_version`. Either can be overridden at runtime.

use tracing::{info, warn};

/// Lowest WSL version this build was verified against.
pub const VERIFIED_MIN: &str = "2.6.3.0";

/// Highest WSL version this build was verified against.
///
/// Keep this in sync with the newest *stable* WSL release. Being slightly
/// behind is harmless: `NewerThanVerified` is accepted, and capabilities are
/// probed at runtime rather than inferred from this number.
pub const VERIFIED_MAX: &str = "3.0.1.0";

/// Replaces the resolved upper bound, e.g. `WSLDASH_WSL_MAX_VERSION=3.1.0.0`.
/// Useful for trying an unreleased WSL build without patching the source.
pub const ENV_MAX_OVERRIDE: &str = "WSLDASH_WSL_MAX_VERSION";

/// Replaces the resolved lower bound.
pub const ENV_MIN_OVERRIDE: &str = "WSLDASH_WSL_MIN_VERSION";

/// Set to `1` / `true` / `yes` to skip the version gate entirely.
pub const ENV_SKIP_CHECK: &str = "WSLDASH_SKIP_WSL_VERSION_CHECK";

/// Set to `1` / `true` / `yes` to restore the old strict behaviour, i.e. warn
/// about anything outside `[min, max]` instead of only about `TooOld`.
pub const ENV_STRICT_CHECK: &str = "WSLDASH_STRICT_WSL_VERSION_CHECK";

/// Outcome of comparing the installed WSL against the accepted window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionVerdict {
    /// Inside the accepted window.
    Verified,
    /// Newer than the window, but accepted: capabilities are probed at runtime,
    /// so an unknown-newer version is handled by feature detection, not by a
    /// version-number guess.
    NewerThanVerified { max: String },
    /// Older than the floor. This is a genuine risk and still warns.
    TooOld { min: String },
}

impl VersionVerdict {
    /// `true` when the app should proceed without showing the compat dialog.
    pub fn is_supported(&self) -> bool {
        match self {
            VersionVerdict::Verified => true,
            VersionVerdict::NewerThanVerified { .. } => true,
            VersionVerdict::TooOld { .. } => false,
        }
    }
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        }
        Err(_) => false,
    }
}

/// `true` when the version gate should be skipped altogether.
pub fn skip_requested() -> bool {
    env_flag(ENV_SKIP_CHECK)
}

/// `true` when the old, strict upper-bound check should be restored.
pub fn strict_mode() -> bool {
    env_flag(ENV_STRICT_CHECK)
}

fn env_version(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && looks_like_version(v))
}

/// Parse a dotted version into comparable numeric segments.
///
/// Non-numeric segments are dropped, so `"2.9.11.0"` -> `[2, 9, 11, 0]`.
pub fn parse_version(s: &str) -> Vec<u64> {
    s.split('.')
        .filter_map(|p| p.trim().parse::<u64>().ok())
        .collect()
}

/// `true` if the string contains at least one dotted numeric version token.
pub fn looks_like_version(s: &str) -> bool {
    !parse_version(s).is_empty()
}

/// Compare two dotted versions segment by segment; missing segments count as 0.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts = parse_version(a);
    let b_parts = parse_version(b);
    let len = a_parts.len().max(b_parts.len());
    for i in 0..len {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        if av != bv {
            return av.cmp(&bv);
        }
    }
    std::cmp::Ordering::Equal
}

/// `true` when `a >= b`.
pub fn version_gte(a: &str, b: &str) -> bool {
    compare_versions(a, b) != std::cmp::Ordering::Less
}

/// `true` when `a <= b`.
pub fn version_lte(a: &str, b: &str) -> bool {
    compare_versions(a, b) != std::cmp::Ordering::Greater
}

/// Resolve the lower bound actually used: the stricter of the built-in floor
/// and the server-provided `since_version`.
pub fn resolve_min(remote_since: &str) -> String {
    if let Some(v) = env_version(ENV_MIN_OVERRIDE) {
        return v;
    }
    let remote_since = remote_since.trim();
    if remote_since.is_empty() || !looks_like_version(remote_since) {
        return VERIFIED_MIN.to_string();
    }
    if version_gte(remote_since, VERIFIED_MIN) {
        remote_since.to_string()
    } else {
        VERIFIED_MIN.to_string()
    }
}

/// Resolve the upper bound actually used: the newer of the built-in ceiling and
/// the server-provided `until_version`.
pub fn resolve_max(remote_until: &str) -> String {
    if let Some(v) = env_version(ENV_MAX_OVERRIDE) {
        return v;
    }
    let remote_until = remote_until.trim();
    if remote_until.is_empty() || !looks_like_version(remote_until) {
        return VERIFIED_MAX.to_string();
    }
    if version_gte(remote_until, VERIFIED_MAX) {
        remote_until.to_string()
    } else {
        VERIFIED_MAX.to_string()
    }
}

/// Compare `current` against an explicit window. Pure function (no env access),
/// so it is directly unit-testable.
pub fn verdict_for(current: &str, min: &str, max: &str) -> VersionVerdict {
    if !looks_like_version(current) {
        // Unparseable versions are handled by `detection_failed` upstream.
        return VersionVerdict::Verified;
    }
    if !version_gte(current, min) {
        return VersionVerdict::TooOld {
            min: min.to_string(),
        };
    }
    if !version_lte(current, max) {
        return VersionVerdict::NewerThanVerified {
            max: max.to_string(),
        };
    }
    VersionVerdict::Verified
}

/// Convenience wrapper: resolves the window from the server values (honouring
/// the environment overrides) and returns the verdict plus the resolved bounds.
pub fn evaluate(current: &str, remote_since: &str, remote_until: &str) -> (VersionVerdict, String, String) {
    let min = resolve_min(remote_since);
    let max = resolve_max(remote_until);

    let verdict = if strict_mode() {
        // Strict mode: anything outside [min, max] is treated as a warning,
        // matching the pre-fork behaviour.
        match verdict_for(current, &min, &max) {
            VersionVerdict::NewerThanVerified { max } => VersionVerdict::TooOld { min: max },
            other => other,
        }
    } else {
        verdict_for(current, &min, &max)
    };

    match &verdict {
        VersionVerdict::Verified => {
            info!("wsl_compat: {} is inside the accepted window [{}, {}]", current, min, max)
        }
        VersionVerdict::NewerThanVerified { max } => warn!(
            "wsl_compat: {} is newer than the verified ceiling {} — accepting it because \
             capabilities are probed at runtime. Set {} to force a warning.",
            current, max, ENV_STRICT_CHECK
        ),
        VersionVerdict::TooOld { min } => warn!(
            "wsl_compat: {} is older than the required minimum {}",
            current, min
        ),
    }

    (verdict, min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_compares_dotted_versions() {
        assert_eq!(parse_version("2.9.11.0"), vec![2, 9, 11, 0]);
        assert_eq!(parse_version("2.7"), vec![2, 7]);
        assert_eq!(compare_versions("2.9.11.0", "2.7.14.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("2.7.14.0", "2.7.14"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("2.7.9.0", "2.7.14.0"), std::cmp::Ordering::Less);
        assert!(version_gte("3.0.1.0", "2.6.3.0"));
        assert!(version_lte("2.6.3.0", "2.6.3.0"));
        assert!(!looks_like_version(""));
        assert!(!looks_like_version("nope"));
    }

    #[test]
    fn accepts_newer_versions_instead_of_warning() {
        // The exact case from issue #131: server ceiling 2.7.14.0, user on 2.9.11.0.
        assert_eq!(
            verdict_for("2.9.11.0", "2.6.3.0", "2.7.14.0"),
            VersionVerdict::NewerThanVerified { max: "2.7.14.0".into() }
        );
        assert!(verdict_for("2.9.11.0", "2.6.3.0", "2.7.14.0").is_supported());
        assert!(verdict_for("3.0.1.0", "2.6.3.0", "3.0.1.0").is_supported());
    }

    #[test]
    fn still_warns_about_too_old_versions() {
        let v = verdict_for("2.4.8.0", "2.6.3.0", "3.0.1.0");
        assert_eq!(v, VersionVerdict::TooOld { min: "2.6.3.0".into() });
        assert!(!v.is_supported());
    }

    #[test]
    fn built_in_ceiling_wins_over_a_stale_server_list() {
        // Server list is older than what this build knows to be fine.
        assert_eq!(resolve_max("2.7.14.0"), VERIFIED_MAX);
        // Server list is newer -> trust it, no rebuild needed.
        assert_eq!(resolve_max("4.1.0.0"), "4.1.0.0");
        // Missing / garbage -> fall back to the built-in value.
        assert_eq!(resolve_max(""), VERIFIED_MAX);
        assert_eq!(resolve_max("unknown"), VERIFIED_MAX);
    }

    #[test]
    fn server_can_raise_the_floor() {
        assert_eq!(resolve_min("2.6.3.0"), VERIFIED_MIN);
        assert_eq!(resolve_min("2.8.0.0"), "2.8.0.0");
        assert_eq!(resolve_min("1.0.0.0"), VERIFIED_MIN);
    }
}
