//! Provenance labels decided in DEC-003 (definitions in `roadmap/docs/master.md`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Who took part in making a thing. Ordered from least to most AI involvement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provenance {
    /// AI was never used for the thing.
    Human,
    /// AI was used and a human took part in at least one step.
    AiAssisted,
    /// Conceived and built entirely by AI, with no human anywhere in the process.
    AiGenerated,
}

impl Provenance {
    /// Label used in commit trailers and reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::AiAssisted => "ai-assisted",
            Self::AiGenerated => "ai-generated",
        }
    }

    /// The label after a human takes part (editing or approving): AI work becomes
    /// AI-Assisted; Human stays Human.
    pub fn after_human_step(self) -> Self {
        match self {
            Self::AiGenerated => Self::AiAssisted,
            other => other,
        }
    }

    /// The label after AI takes part: Human work becomes AI-Assisted; AI work keeps
    /// its label. Labels never return to Human or AI-Generated.
    pub fn after_ai_step(self) -> Self {
        match self {
            Self::Human => Self::AiAssisted,
            other => other,
        }
    }

    /// Label of an aggregate (a map, a release): the most AI-involved part.
    pub fn combine(parts: impl IntoIterator<Item = Self>) -> Option<Self> {
        parts.into_iter().max()
    }
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::Provenance::*;
    use super::*;

    #[test]
    fn transitions_are_monotonic() {
        assert_eq!(AiGenerated.after_human_step(), AiAssisted);
        assert_eq!(AiAssisted.after_human_step(), AiAssisted);
        assert_eq!(Human.after_human_step(), Human);
        assert_eq!(Human.after_ai_step(), AiAssisted);
        assert_eq!(AiAssisted.after_ai_step(), AiAssisted);
        assert_eq!(AiGenerated.after_ai_step(), AiGenerated);
        for start in [Human, AiAssisted, AiGenerated] {
            // Once a human and an AI have both taken part, nothing returns to a pure label.
            assert_eq!(start.after_ai_step().after_human_step(), AiAssisted);
            assert_eq!(start.after_human_step().after_ai_step().after_human_step(), AiAssisted);
        }
    }

    #[test]
    fn aggregate_takes_most_ai_involved_part() {
        assert_eq!(Provenance::combine([Human, AiAssisted, Human]), Some(AiAssisted));
        assert_eq!(Provenance::combine([Human, AiGenerated]), Some(AiGenerated));
        assert_eq!(Provenance::combine([]), None);
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(AiGenerated.label(), "ai-generated");
        assert_eq!(AiAssisted.to_string(), "ai-assisted");
        assert_eq!(Human.label(), "human");
    }
}
