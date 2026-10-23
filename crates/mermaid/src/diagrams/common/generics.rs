//! Generic type notation with tildes. Port of Pele's `src/diagrams/common/generics.ts`.

fn tildes(text: &str) -> usize {
    text.matches('~').count()
}

/// Pairs tildes from the outside in: the first with the last, the second with the one before it.
fn pair_tildes(text: &str) -> String {
    let count = tildes(text);
    if count <= 1 {
        return text.to_owned();
    }

    let lead = !count.is_multiple_of(2) && text.starts_with('~');
    let mut chars: Vec<char> = text.chars().collect();
    let at: Vec<usize> =
        chars.iter().enumerate().skip(usize::from(lead)).filter(|&(_, &c)| c == '~').map(|(i, _)| i).collect();
    for (&lo, &hi) in at.iter().zip(at.iter().rev()).take(at.len() / 2) {
        if let Some(c) = chars.get_mut(lo) {
            *c = '<';
        }
        if let Some(c) = chars.get_mut(hi) {
            *c = '>';
        }
    }
    chars.into_iter().collect()
}

/// Turns `List~int~` into `List<int>`, with the quirks of Mermaid's `parseGenericTypes` around
/// commas.
pub(crate) fn parse_generic_types(input: &str) -> String {
    if !input.contains('~') {
        return input.to_owned();
    }

    // `input.split(/(,)/)`: the pieces between commas, with each comma kept as a piece.
    let mut sets: Vec<&str> = Vec::new();
    for (i, piece) in input.split(',').enumerate() {
        if i > 0 {
            sets.push(",");
        }
        sets.push(piece);
    }

    let mut output: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(&set) = sets.get(i) {
        let joined = if set == "," && i > 0 {
            match (sets.get(i - 1), sets.get(i + 1)) {
                (Some(&before), Some(&after)) if tildes(before) == 1 && tildes(after) == 1 => {
                    i += 1;
                    output.pop();
                    let mut joined = String::from(before);
                    joined.push(',');
                    joined.push_str(after);
                    Some(joined)
                }
                _ => None,
            }
        } else {
            None
        };
        output.push(pair_tildes(joined.as_deref().unwrap_or(set)));
        i += 1;
    }
    output.concat()
}
