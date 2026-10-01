// SPDX-FileCopyrightText: Copyright (c) 2026 owu <wqh@live.com>
// SPDX-License-Identifier: GPL-3.0-only

pub mod icon_mapper;
pub mod logging;
pub mod registry;
pub mod system;
pub mod wsl_config;
pub mod theme;

/// Cut a string to at most `max_bytes` bytes without splitting a UTF-8 character.
///
/// `&s[..n]` panics when `n` lands inside a multi-byte character, which is easy
/// to hit here because WSL and Windows both emit localized text (a Chinese help
/// or error message is 3 bytes per glyph). Always prefer this over raw byte
/// slicing when the input may be non-ASCII.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Same as [`truncate_bytes`], appending `...` only when something was dropped.
pub fn truncate_bytes_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("{}...", truncate_bytes(s, max_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_bytes_never_splits_a_character() {
        // "中" is 3 bytes; cutting at 1 or 2 bytes must back off to 0.
        assert_eq!(truncate_bytes("中文", 1), "");
        assert_eq!(truncate_bytes("中文", 2), "");
        assert_eq!(truncate_bytes("中文", 3), "中");
        assert_eq!(truncate_bytes("中文", 4), "中");
        assert_eq!(truncate_bytes("中文", 6), "中文");
        // ASCII stays byte-oriented.
        assert_eq!(truncate_bytes("abcdef", 4), "abcd");
        assert_eq!(truncate_bytes("abcdef", 99), "abcdef");
    }

    #[test]
    fn truncate_ellipsis_only_when_needed() {
        assert_eq!(truncate_bytes_ellipsis("abc", 10), "abc");
        assert_eq!(truncate_bytes_ellipsis("abcdef", 3), "abc...");
        assert_eq!(truncate_bytes_ellipsis("中文字", 4), "中...");
    }
}
