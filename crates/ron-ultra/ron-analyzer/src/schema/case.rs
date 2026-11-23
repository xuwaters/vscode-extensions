//! serde's `rename_all` rules, applied the way serde applies them: field
//! names are assumed `snake_case` and variant names `PascalCase`.

pub fn rename_field(rule: &str, field: &str) -> String {
    match rule {
        "lowercase" | "snake_case" => field.to_string(),
        "UPPERCASE" | "SCREAMING_SNAKE_CASE" => field.to_ascii_uppercase(),
        "PascalCase" => pascal(field),
        "camelCase" => {
            let p = pascal(field);
            let mut chars = p.chars();
            match chars.next() {
                Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
                None => p,
            }
        }
        "kebab-case" => field.replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => field.to_ascii_uppercase().replace('_', "-"),
        _ => field.to_string(),
    }
}

pub fn rename_variant(rule: &str, variant: &str) -> String {
    match rule {
        "lowercase" => variant.to_ascii_lowercase(),
        "UPPERCASE" => variant.to_ascii_uppercase(),
        "PascalCase" => variant.to_string(),
        "camelCase" => {
            let mut chars = variant.chars();
            match chars.next() {
                Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
        "snake_case" => snake(variant),
        "SCREAMING_SNAKE_CASE" => snake(variant).to_ascii_uppercase(),
        "kebab-case" => snake(variant).replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => snake(variant).to_ascii_uppercase().replace('_', "-"),
        _ => variant.to_string(),
    }
}

fn pascal(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn snake(pascal: &str) -> String {
    let mut out = String::new();
    for (i, c) in pascal.char_indices() {
        if i > 0 && c.is_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Edit distance, for "did you mean" hints.
pub fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest candidate within a small edit distance (or differing only
/// in case).
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = name.chars().count().div_ceil(3).clamp(1, 3);
    candidates
        .into_iter()
        .map(|c| (if c.eq_ignore_ascii_case(name) { 0 } else { distance(name, c) }, c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields() {
        assert_eq!(rename_field("camelCase", "window_size"), "windowSize");
        assert_eq!(rename_field("PascalCase", "window_size"), "WindowSize");
        assert_eq!(rename_field("kebab-case", "window_size"), "window-size");
        assert_eq!(rename_field("SCREAMING_SNAKE_CASE", "window_size"), "WINDOW_SIZE");
    }

    #[test]
    fn variants() {
        assert_eq!(rename_variant("snake_case", "VeryHard"), "very_hard");
        assert_eq!(rename_variant("camelCase", "VeryHard"), "veryHard");
        assert_eq!(rename_variant("SCREAMING-KEBAB-CASE", "VeryHard"), "VERY-HARD");
        assert_eq!(rename_variant("lowercase", "VeryHard"), "veryhard");
    }

    #[test]
    fn suggestions() {
        assert_eq!(suggest("widht", ["width", "height"]), Some("width"));
        assert_eq!(suggest("EASY", ["Easy", "Hard"]), Some("Easy"));
        assert_eq!(suggest("zzz", ["width"]), None);
    }
}
