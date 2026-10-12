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
    /// `T<n> M6`, for a machine with a changer or a macro.
    M6,
    /// A program pause with a comment, for changing by hand.
    Pause,
}

/// How arc centres are written.
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
#[serde(deny_unknown_fields)]
pub struct Format {
    /// Decimal places for axis words.
    pub axis_decimals: u8,
    /// Decimal places for feed words.
    pub feed_decimals: u8,
    /// Decimal places for axis words in a program written in inches.
    #[serde(default = "four")]
    pub inch_axis_decimals: u8,
    /// Whether to drop trailing zeros, so `10.500` becomes `10.5`.
    #[serde(default)]
    pub strip_trailing_zeros: bool,
    /// Whether a number with its trailing zeros dropped keeps its point: `10.`
    /// rather than `10`, as Fusion writes them.
    #[serde(default)]
    pub keep_point: bool,
}

fn four() -> u8 {
    4
}

/// What units a program is written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgramUnits {
    /// Millimetres (`G21`), as the engine works.
    #[default]
    Mm,
    /// Inches (`G20`): every length and feed converted.
    Inch,
}

/// Line numbering.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Display name.
    pub name: String,
    /// File extension, without the dot.
    pub extension: String,
    /// How comments are wrapped, as a pair of strings.
    pub comment: (String, String),
    /// The characters a comment may hold, letters in either case; any other is
    /// left out, as Fusion's posts filter them. Absent, any may.
    #[serde(default)]
    pub comment_chars: Option<String>,
    /// Whether comments are written in capitals.
    #[serde(default)]
    pub comment_upper: bool,
    /// Number formatting.
    pub format: Format,
    /// Line numbering.
    pub numbering: Numbering,
    /// Modal suppression.
    pub modal: Modal,
    /// Arc centre style.
    pub arc_style: ArcStyle,
    /// The units its programs are written in.
    #[serde(default)]
    pub units: ProgramUnits,
    /// Tool change style.
    pub tool_change: ToolChangeStyle,
    /// Whether the first move down after a tool change takes up the tool's
    /// length offset, `G43 Z… H<tool>`, as Fusion's posts write it. Off, the
    /// controller applies the offset itself, or has none.
    #[serde(default)]
    pub length_offset: bool,
    /// Whether the controller takes canned drilling cycles (`G81`, `G82`, `G83`,
    /// `G80`). Off, a drilling operation is posted as its moves.
    #[serde(default)]
    pub drill_cycles: bool,
    /// Lines emitted before anything else. `{name}` is replaced.
    pub header: Vec<String>,
    /// Lines that take the tool straight up clear of everything, as far as the
    /// machine goes, such as `G28 G91 Z0`: before each tool change and before the
    /// program ends. Empty, the tool is not sent up, and the first move after a
    /// tool change rises to its height before it moves across.
    #[serde(default)]
    pub safe_retract: Vec<String>,
    /// Lines that send the machine home across, such as `G28 G91 X0 Y0`, after
    /// the last safe retract and before the footer.
    #[serde(default)]
    pub end_home: Vec<String>,
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

/// Millimetres in an inch.
const MM_PER_INCH: f64 = 25.4;

/// The profiles compiled in, by name: each a file of its own in `profiles/`.
const BUILTIN: &[(&str, &str)] = &[
    ("masso", include_str!("../profiles/masso.toml")),
    ("masso-inch", include_str!("../profiles/masso-inch.toml")),
    ("grbl", include_str!("../profiles/grbl.toml")),
    (
        "centroid-avid",
        include_str!("../profiles/centroid-avid.toml"),
    ),
    (
        "centroid-avid-inch",
        include_str!("../profiles/centroid-avid-inch.toml"),
    ),
];

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
        let name = name.to_ascii_lowercase();
        BUILTIN
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, text)| Self::from_toml(text))
    }

    /// Names of the compiled-in profiles.
    pub fn builtin_names() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|(name, _)| *name)
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
        if self.format.inch_axis_decimals > 9 {
            return Err(ProfileError::BadValue {
                field: "format.inch_axis_decimals",
                reason: format!("{} exceeds 9", self.format.inch_axis_decimals),
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
        if self.comment.0.is_empty() {
            return Err(ProfileError::BadValue {
                field: "comment",
                reason: "a comment needs something to open it".into(),
            });
        }
        if self.comment_chars.as_deref() == Some("") {
            return Err(ProfileError::BadValue {
                field: "comment_chars",
                reason: "would leave every comment empty; leave it out to allow any character"
                    .into(),
            });
        }
        Ok(())
    }

    /// Format a value for an axis word.
    pub fn axis(&self, v: f64) -> String {
        self.number(v, self.format.axis_decimals)
    }

    /// Format a length in millimetres for an axis word, in inches if `inches`.
    pub fn length(&self, mm: f64, inches: bool) -> String {
        if inches {
            self.number(mm / MM_PER_INCH, self.format.inch_axis_decimals)
        } else {
            self.axis(mm)
        }
    }

    /// Format a feed in mm/min for a feed word, in in/min if `inches`.
    pub fn feed_in(&self, mm_per_min: f64, inches: bool) -> String {
        self.feed(if inches {
            mm_per_min / MM_PER_INCH
        } else {
            mm_per_min
        })
    }

    /// Format a value for a feed word.
    pub fn feed(&self, v: f64) -> String {
        self.number(v, self.format.feed_decimals)
    }

    /// Format a time in seconds for a dwell word: to the millisecond, as
    /// Fusion's posts write one, whatever the feed's places.
    pub fn seconds(&self, v: f64) -> String {
        self.number(v, 3)
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
            if s.ends_with('.') && !self.format.keep_point {
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
        format!("{}{}{}", self.comment.0, self.escaped(text), self.comment.1)
    }

    /// `text` made safe to stand inside a comment: its delimiters substituted, as
    /// [`comment`](Self::comment) says, and control characters, a newline above
    /// all, made spaces. An empty delimiter, as a comment to the line's end has
    /// for its close, is nothing to substitute. Then in capitals, and without
    /// what the profile's comments may not hold, where it says so.
    pub fn escaped(&self, text: &str) -> String {
        let mut safe = text.to_string();
        for (delimiter, with) in [(&self.comment.0, "["), (&self.comment.1, "]")] {
            if !delimiter.is_empty() {
                safe = safe.replace(delimiter.as_str(), with);
            }
        }
        if self.comment_upper {
            safe = safe.to_uppercase();
        }
        let allowed = |c: char| {
            self.comment_chars
                .as_deref()
                .is_none_or(|chars| chars.chars().any(|a| a.eq_ignore_ascii_case(&c)))
        };
        safe.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .filter(|&c| allowed(c))
            .collect()
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

    /// Masso in inches is its own file, and the Masso profile in everything but
    /// its name and units.
    #[test]
    fn masso_inch_is_masso_in_inches() {
        let masso = Profile::builtin("masso").unwrap().unwrap();
        let inch = Profile::builtin("masso-inch").unwrap().unwrap();
        let expected = Profile {
            name: "Masso (inch)".into(),
            units: ProgramUnits::Inch,
            ..masso
        };
        assert_eq!(inch, expected);
    }

    /// The Avid profile in inches is its own file, and the millimetre one in
    /// everything but its name, units and feed, which Fusion's Avid post writes
    /// to a tenth in inches and to a whole number in millimetres.
    #[test]
    fn centroid_avid_inch_is_centroid_avid_in_inches() {
        let mm = Profile::builtin("centroid-avid").unwrap().unwrap();
        let inch = Profile::builtin("centroid-avid-inch").unwrap().unwrap();
        let expected = Profile {
            name: "Centroid CNC12 (Avid CNC, inch)".into(),
            units: ProgramUnits::Inch,
            format: Format {
                feed_decimals: 1,
                ..mm.format.clone()
            },
            ..mm
        };
        assert_eq!(inch, expected);
    }

    /// Avid as Fusion's Avid post writes it: no trailing zeros and no bare
    /// point, and comments in capitals with only what that post lets through.
    #[test]
    fn centroid_avid_writes_numbers_and_comments_as_fusion_does() {
        let p = Profile::builtin("centroid-avid").unwrap().unwrap();
        assert_eq!(p.axis(10.0), "10");
        assert_eq!(p.axis(2.25), "2.25");
        assert_eq!(p.feed(1200.4), "1200");
        assert_eq!(p.length(25.4 * 3.46084, true), "3.4608");
        assert_eq!(p.comment("6mm ball (carbide) #2"), "(6MM BALL CARBIDE 2)");
        assert_eq!(p.comment("rough: top/side"), "(ROUGH TOPSIDE)");
    }

    /// Masso as Fusion's post writes it: rounded to the places given, trailing
    /// zeros dropped and the point kept; inches to four places.
    #[test]
    fn rounds_to_the_stated_precision() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        assert_eq!(p.axis(1.23456789), "1.235");
        assert_eq!(p.axis(10.0), "10.");
        assert_eq!(p.axis(2.2), "2.2");
        assert_eq!(p.feed(150.0), "150.");
        assert_eq!(p.length(25.4 * 3.46084, true), "3.4608");
        assert_eq!(p.feed_in(3810.0, true), "150.");
        let mut plain = p.clone();
        plain.format.strip_trailing_zeros = false;
        assert_eq!(plain.axis(10.0), "10.000");
        let grbl = Profile::builtin("grbl").unwrap().unwrap();
        assert_eq!(grbl.axis(10.0), "10", "GRBL drops the point too");
        let inch = Profile::builtin("masso-inch").unwrap().unwrap();
        assert_eq!(inch.units, ProgramUnits::Inch);
    }

    /// A negative zero is the same position as zero and reads like a fault.
    #[test]
    fn never_emits_negative_zero() {
        let p = Profile::builtin("masso").unwrap().unwrap();
        assert_eq!(p.axis(-0.0), "0.");
        assert_eq!(p.axis(-0.0001), "0.");
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

    /// A comment to the line's end has no close to substitute; substituting
    /// nothing would put a bracket between every character.
    #[test]
    fn a_comment_to_the_line_end_reads_as_written() {
        let mut p = Profile::builtin("grbl").unwrap().unwrap();
        p.comment = (";".into(), String::new());
        assert_eq!(p.comment("fit tool 3; then go"), ";fit tool 3[ then go");
    }

    /// Filtering keeps letters in whichever case they were written, and a
    /// newline, made a space, survives only where a space may.
    #[test]
    fn comment_chars_match_either_case() {
        let mut p = Profile::builtin("masso").unwrap().unwrap();
        p.comment_chars = Some("abc ".into());
        assert_eq!(
            p.comment(&format!("AbCd{}cab!", char::from(10))),
            "(AbC cab)"
        );
        p.comment_chars = Some("abc".into());
        assert_eq!(p.comment(&format!("ab{}c", char::from(10))), "(abc)");
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
