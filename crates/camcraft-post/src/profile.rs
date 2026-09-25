//! Machine profiles: TOML, not Rust.
//!
//! Adding a machine must never mean recompiling, so a dialect is data. A profile
//! says how to format numbers, whether to suppress modal words, how a tool change is
//! expressed, and what goes at the top and bottom of a file.
//!
//! # Modal suppression tracks what was *emitted*
//!
//! Leaving out an axis word because it has not changed is correct only if the
//! machine already has the value, which means tracking what the renderer actually
//! wrote — not what it intended. The distinction matters at the first move of a
//! program, where nothing has been written yet and every word must appear.
//!
//! # Rounding is a safety property
//!
//! Coordinates are rounded once, and the rounded value is what the modal comparison
//! uses. Comparing unrounded values while emitting rounded ones lets a move be
//! suppressed as "unchanged" when the machine would have seen a different number.

use serde::Deserialize;

/// How a tool change is expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChangeStyle {
    /// `M6 T<n>`, for a machine with a changer or a macro.
    M6,
    /// A program pause with a comment, for changing by hand.
    Pause,
}

/// How arc centres are written. Reserved; nothing emits arcs yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcStyle {
    /// Incremental centre offsets, `I`/`J`.
    Ij,
    /// Radius, `R`.
    R,
}

/// Number formatting.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Format {
    /// Decimal places for axis words.
    pub axis_decimals: u8,
    /// Decimal places for feed words.
    pub feed_decimals: u8,
    /// Whether to drop trailing zeros, so `10.500` becomes `10.5`.
    #[serde(default)]
    pub strip_trailing_zeros: bool,
}

/// Line numbering.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Numbering {
    /// Whether to number lines at all.
    pub enabled: bool,
    /// First number.
    #[serde(default = "one")]
    pub start: u32,
    /// Step between numbers.
    #[serde(default = "one")]
    pub increment: u32,
}

fn one() -> u32 {
    1
}

/// Modal behaviour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Modal {
    /// Omit `G0`/`G1` when the motion mode has not changed.
    pub suppress_motion: bool,
    /// Omit an axis word whose value has not changed.
    pub suppress_axes: bool,
    /// Omit `F` when the feed has not changed.
    pub suppress_feed: bool,
}

/// One machine dialect.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Profile {
    /// Display name.
    pub name: String,
    /// File extension, without the dot.
    pub extension: String,
    /// How comments are wrapped, as a pair of strings.
    pub comment: (String, String),
    /// Number formatting.
    pub format: Format,
    /// Line numbering.
    pub numbering: Numbering,
    /// Modal suppression.
    pub modal: Modal,
    /// Arc centre style. Reserved.
    pub arc_style: ArcStyle,
    /// Tool change style.
    pub tool_change: ToolChangeStyle,
    /// Whether the controller takes canned drilling cycles (`G81`, `G82`, `G83`,
    /// `G80`). Off, a drilling operation is posted as its moves.
    #[serde(default)]
    pub drill_cycles: bool,
    /// Lines emitted before anything else. `{name}` is replaced.
    pub header: Vec<String>,
    /// Lines emitted at the end.
    pub footer: Vec<String>,
}

/// A profile that could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The TOML was malformed or missing a field.
    #[error("profile is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// A value was outside its usable range.
    #[error("{field} is not usable: {reason}")]
    BadValue {
        /// Which field.
        field: &'static str,
        /// Why.
        reason: String,
    },
}

/// The Masso profile, compiled in.
pub const MASSO: &str = include_str!("../profiles/masso.toml");
/// The GRBL profile, compiled in.
pub const GRBL: &str = include_str!("../profiles/grbl.toml");

impl Profile {
    /// Parse a profile from TOML.
    pub fn from_toml(text: &str) -> Result<Self, ProfileError> {
        let p: Profile = toml::from_str(text)?;
        p.validate()?;
        Ok(p)
    }

    /// Load a profile from a file.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ProfileError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml(&text)
    }

    /// One of the compiled-in profiles, by name.
    pub fn builtin(name: &str) -> Option<Result<Self, ProfileError>> {
        match name.to_ascii_lowercase().as_str() {
            "masso" => Some(Self::from_toml(MASSO)),
            "grbl" => Some(Self::from_toml(GRBL)),
            _ => None,
        }
    }

    /// Names of the compiled-in profiles.
    pub fn builtin_names() -> &'static [&'static str] {
        &["masso", "grbl"]
    }

    fn validate(&self) -> Result<(), ProfileError> {
        // 9 places is past f64's useful precision at machine scale; past that the
        // output is noise dressed as precision.
        if self.format.axis_decimals > 9 {
            return Err(ProfileError::BadValue {
                field: "format.axis_decimals",
                reason: format!("{} exceeds 9", self.format.axis_decimals),
            });
        }
        if self.format.feed_decimals > 9 {
            return Err(ProfileError::BadValue {
                field: "format.feed_decimals",
                reason: format!("{} exceeds 9", self.format.feed_decimals),
            });
        }
        if self.numbering.enabled && self.numbering.increment == 0 {
            return Err(ProfileError::BadValue {
                field: "numbering.increment",
                reason: "zero would repeat the same line number forever".into(),
            });
        }
        if self.extension.is_empty() {
            return Err(ProfileError::BadValue {
                field: "extension",
                reason: "must not be empty".into(),
            });
        }
        Ok(())
    }

    /// Format a value for an axis word.
    pub fn axis(&self, v: f64) -> String {
        self.number(v, self.format.axis_decimals)
    }

    /// Format a value for a feed word.
    pub fn feed(&self, v: f64) -> String {
        self.number(v, self.format.feed_decimals)
    }

    fn number(&self, v: f64, decimals: u8) -> String {
        let mut s = format!("{v:.*}", decimals as usize);
        // "-0.000" is the same position as "0.000" and reads like an error.
        if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
            s.remove(0);
        }
        if self.format.strip_trailing_zeros && s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }

    /// Wrap text as a comment in this dialect.
    ///
    /// Both delimiters are substituted, not just the closing one. A stray close
    /// would end the comment early and leave the rest of the line to be executed;
    /// a stray open leaves an unbalanced comment that several controllers reject or
    /// swallow the next block into. Square brackets keep the text readable.
    pub fn comment(&self, text: &str) -> String {
        let safe: String = text
            .replace(&self.comment.0, "[")
            .replace(&self.comment.1, "]")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        format!("{}{}{}", self.comment.0, safe, self.comment.1)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn both_builtins_parse() {
        for name in Profile::builtin_names() {
            let p = Profile::builtin(name).unwrap().unwrap();
            assert!(!p.name.is_empty());
            assert!(!p.extension.is_empty());
        }
        assert!(Profile::builtin("nonesuch").is_none());
    }

    #[test]
    fn rounds_to_the_stated_precision() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        assert_eq!(p.axis(1.23456789), "1.235");
        assert_eq!(p.axis(10.0), "10.000");
    }

    /// A negative zero is the same position as zero and reads like a fault.
    #[test]
    fn never_emits_negative_zero() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        assert_eq!(p.axis(-0.0), "0.000");
        assert_eq!(p.axis(-0.0001), "0.000");
        assert_eq!(p.axis(-0.001), "-0.001");
    }

    /// A stray close-paren ends the comment and leaves the rest of the line to be
    /// executed; a stray open leaves it unbalanced. Neither may survive.
    #[test]
    fn comments_cannot_escape_themselves() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        let c = p.comment("tool (6mm) ball");
        assert_eq!(c.matches('(').count(), 1, "only the opening delimiter: {c}");
        assert_eq!(c.matches(')').count(), 1, "only the closing delimiter: {c}");
        assert!(c.starts_with('(') && c.ends_with(')'));
        assert!(c.contains("[6mm]"), "text should stay readable: {c}");
    }

    /// A newline inside a comment would end the block and execute the remainder.
    #[test]
    fn comments_cannot_contain_newlines() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        let c = p.comment(&format!("first{}M3 S30000", char::from(10)));
        assert!(!c.contains(char::from(10)), "newline survived: {c:?}");
    }

    #[test]
    fn rejects_unusable_values() {
        let bad = r#"
            name = "x"
            extension = "nc"
            comment = ["(", ")"]
            arc_style = "ij"
            tool_change = "m6"
            header = []
            footer = []
            [format]
            axis_decimals = 20
            feed_decimals = 1
            [numbering]
            enabled = false
            [modal]
            suppress_motion = true
            suppress_axes = true
            suppress_feed = true
        "#;
        assert!(matches!(
            Profile::from_toml(bad),
            Err(ProfileError::BadValue {
                field: "format.axis_decimals",
                ..
            })
        ));
    }

    #[test]
    fn strip_trailing_zeros_is_per_profile() {
        let masso = Profile::builtin("masso").unwrap().unwrap();
        let grbl = Profile::builtin("grbl").unwrap().unwrap();
        // Whatever each chooses, it must be self-consistent.
        for p in [&masso, &grbl] {
            let s = p.axis(10.5);
            if p.format.strip_trailing_zeros {
                assert!(!s.ends_with('0'), "{} kept a trailing zero: {s}", p.name);
            } else {
                assert_eq!(
                    s.split('.').nth(1).map(str::len),
                    Some(p.format.axis_decimals as usize)
                );
            }
        }
    }
}
