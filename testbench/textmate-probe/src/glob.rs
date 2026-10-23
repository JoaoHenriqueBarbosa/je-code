//! Subconjunto do glob do VS Code usado em `filenamePatterns`: `*`, `?`, `**` e `{a,b}`, sem
//! diferenciar maiúsculas de minúsculas.

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close) = pattern[open..].find('}').map(|c| c + open) else {
        return vec![pattern.to_string()];
    };
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close]
        .split(',')
        .flat_map(|alt| expand_braces(&format!("{head}{alt}{tail}")))
        .collect()
}

fn segment_matches(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| segment_matches(rest, &text[skip..])),
        Some(('?', rest)) => !text.is_empty() && segment_matches(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && segment_matches(rest, &text[1..]),
    }
}

fn segments_match(pattern: &[&str], text: &[&str]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((&"**", rest)) => (0..=text.len()).any(|skip| segments_match(rest, &text[skip..])),
        Some((seg, rest)) => text.split_first().is_some_and(|(t, text_rest)| {
            let seg: Vec<char> = seg.chars().collect();
            let t: Vec<char> = t.chars().collect();
            segment_matches(&seg, &t) && segments_match(rest, text_rest)
        }),
    }
}

pub fn matches(pattern: &str, target: &str) -> bool {
    let target = target.to_lowercase();
    let text: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    expand_braces(&pattern.to_lowercase()).iter().any(|p| {
        let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        segments_match(&segs, &text)
    })
}
