pub mod anchor;
pub mod compare;

use crate::config::Segment;

/// Printed once when a resolved segment isn't a decision input (docs/DESIGN.md §2.2) —
/// shared so every segment-taking subcommand says the same thing instead of repeating it.
pub fn note_if_not_decision_input(name: &str, segment: &Segment) {
    if !segment.decision_input {
        println!(
            "Note: segment `{name}` is not a decision input (docs/DESIGN.md §2.2) — reporting only."
        );
    }
}
