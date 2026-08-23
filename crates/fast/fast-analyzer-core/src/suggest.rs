//! Nearest-name suggestions. `strsim` replaces `didyoumean2`; the parameters
//! were chosen against fixtures (see the tests) — a wrong suggestion is worse
//! than none, so the threshold errs high.

/// The best near-miss for `input` among `candidates`, or `None` when nothing
/// is close enough to say "did you mean" with a straight face.
pub fn nearest<'a>(input: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let input_lower = input.to_ascii_lowercase();
    let mut best: Option<(&str, f64)> = None;
    for candidate in candidates {
        if candidate == input {
            continue;
        }
        let score = strsim::jaro_winkler(&input_lower, &candidate.to_ascii_lowercase());
        match best {
            Some((_, s)) if s >= score => {}
            _ => best = Some((candidate, score)),
        }
    }
    let (name, score) = best?;
    // 0.84 keeps one-edit typos in short names ("findInpt" → "findInput",
    // "aria-pressd" → "aria-pressed") and rejects unrelated names.
    (score >= 0.84).then_some(name)
}

/// The message tail, honouring `dontShowSuggestions`.
pub fn did_you_mean(suggestion: Option<&str>, dont_show: bool) -> String {
    match suggestion {
        Some(name) if !dont_show => format!(" Did you mean '{name}'?"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_edit_typos_are_found() {
        let members = ["findInput", "rowInput", "tableEl", "viewportEl"];
        assert_eq!(nearest("findInpt", members.iter().copied()), Some("findInput"));
        assert_eq!(nearest("tabelEl", members.iter().copied()), Some("tableEl"));
    }

    #[test]
    fn attribute_typos() {
        let attrs = ["aria-pressed", "aria-label", "class", "tabindex"];
        assert_eq!(nearest("aria-pressd", attrs.iter().copied()), Some("aria-pressed"));
        assert_eq!(nearest("clas", attrs.iter().copied()), Some("class"));
    }

    #[test]
    fn unrelated_names_get_no_suggestion() {
        let members = ["findInput", "rowInput"];
        assert_eq!(nearest("zebra", members.iter().copied()), None);
        assert_eq!(nearest("x", members.iter().copied()), None);
    }

    #[test]
    fn exact_match_is_not_a_suggestion() {
        assert_eq!(nearest("class", ["class"].iter().copied()), None);
    }
}
