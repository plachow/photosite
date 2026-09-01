//! Asking a vision model to describe a photograph.
//!
//! The model runs on this machine, through [Ollama](https://ollama.com), and
//! that is the whole shape of the feature: a photograph goes to it as pixels
//! and the answer comes back as JSON. **A model on somebody else's computer
//! is a different promise entirely**, which is why the address defaults to
//! localhost and why nothing here ships a TLS stack.
//!
//! Two things make the answers usable rather than merely impressive.
//!
//! **The reply is constrained to a schema.** Ollama will hold a model to a
//! JSON shape, so the answer is always machine-readable rather than prose
//! that has to be picked apart with a regular expression and an apology.
//!
//! **The place is told to the model, never asked of it.** A local model
//! shown a coastline will name a country with total confidence and be wrong.
//! So the coordinates are resolved against the offline gazetteer first and
//! the model is handed the answer as a fact — and where the position is
//! itself doubtful, it is handed the doubt too.

pub mod base64;
mod ollama;
pub mod prompt;

pub use ollama::{DEFAULT_ENDPOINT, Insights, describe, models};

/// What to do about a photograph that has already been described.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Fill in what is empty and leave the rest alone.
    ///
    /// The default, and the reason a run over a thousand photographs can be
    /// interrupted and started again: everything already described is
    /// skipped, so the second run picks up where the first stopped.
    #[default]
    FillEmpty,
    Overwrite,
}

impl Mode {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::FillEmpty => "ai-fill-empty",
            Self::Overwrite => "ai-overwrite",
        }
    }
}

/// Is this photograph already done?
pub fn should_skip(mode: Mode, title: Option<&str>, description: Option<&str>) -> bool {
    mode == Mode::FillEmpty
        && title.is_some_and(|text| !text.trim().is_empty())
        && description.is_some_and(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of what makes an interrupted run over a thousand
    /// photographs restartable.
    #[test]
    fn a_photograph_already_described_is_skipped_when_filling_in_the_empty_ones() {
        assert!(should_skip(Mode::FillEmpty, Some("A title"), Some("Words")));
        assert!(!should_skip(Mode::FillEmpty, Some("A title"), None));
        assert!(!should_skip(Mode::FillEmpty, None, Some("Words")));
        assert!(!should_skip(Mode::FillEmpty, None, None));
    }

    /// A field holding nothing but spaces has not been filled in.
    #[test]
    fn a_title_of_spaces_is_not_a_title() {
        assert!(!should_skip(Mode::FillEmpty, Some("   "), Some("Words")));
    }

    #[test]
    fn overwriting_means_overwriting() {
        assert!(!should_skip(
            Mode::Overwrite,
            Some("A title"),
            Some("Words")
        ));
    }
}
