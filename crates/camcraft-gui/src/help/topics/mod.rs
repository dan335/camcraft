//! The help for every input, one file per panel.
//!
//! How to write one:
//!
//! - The title is the field's label, as the panel shows it.
//! - The text is a sentence or two, at most 160 characters: what the field does,
//!   then one thing worth knowing, such as a rule of thumb or what leaving it
//!   empty does. Plain words, no units: the field shows them.
//! - A figure only where a sketch says more than the words: most feeds, names
//!   and notes have none.
//!
//! A topic whose meaning is the same wherever it shows is shared: the preset's
//! Stepover is the operation's.

/// Declares topics, each `NAME: "Title", "Text"` and optionally a figure, and,
/// for the tests, `ALL` of them.
macro_rules! topics {
    ($($name:ident: $title:literal, $text:literal $(, $figure:expr)?;)*) => {
        $(
            #[doc = concat!("Help for ", $title, ".")]
            pub const $name: $crate::help::Topic = $crate::help::Topic {
                title: $title,
                text: $text,
                figure: topics!(@figure $($figure)?),
            };
        )*
        /// Every topic here.
        #[cfg(test)]
        pub const ALL: &[&$crate::help::Topic] = &[$(&$name),*];
    };
    (@figure) => { None };
    (@figure $figure:expr) => { Some($figure) };
}

pub mod app;
pub mod feeds;
pub mod geometry;
pub mod heights;
pub mod library;
pub mod linking;
pub mod model;
pub mod passes;
pub mod setup;
pub mod stock;

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> impl Iterator<Item = &'static crate::help::Topic> {
        [
            app::ALL,
            feeds::ALL,
            geometry::ALL,
            heights::ALL,
            library::ALL,
            linking::ALL,
            model::ALL,
            passes::ALL,
            setup::ALL,
            stock::ALL,
        ]
        .into_iter()
        .flatten()
        .copied()
    }

    /// Every topic has a name, and says what it does in a sentence or two that
    /// end properly: short enough to read at a glance.
    #[test]
    fn every_topic_is_short_and_finished() {
        for t in every() {
            assert!(!t.title.trim().is_empty(), "a topic has no title");
            let n = t.text.chars().count();
            assert!(
                (1..=160).contains(&n),
                "'{}' is {n} characters: keep it to 160",
                t.title
            );
            assert!(
                t.text.ends_with('.'),
                "'{}' should end with a full stop",
                t.title
            );
            assert!(!t.text.contains("  "), "'{}' has a double space", t.title);
        }
    }
}
