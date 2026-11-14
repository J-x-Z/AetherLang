//! Shared literal escapes for Core and Script. Strings contain UTF-8; hex
//! escapes denote ASCII bytes, while Unicode escapes denote scalar values.

/// Decode after an already-consumed backslash. Advance only over the escape,
/// leaving a malformed delimiter/newline available to the caller's recovery.
pub(crate) fn decode_escape(source: &[char], pos: &mut usize) -> Result<char, &'static str> {
    let Some(&kind) = source.get(*pos) else { return Err("incomplete escape sequence"); };
    if matches!(kind, '\n' | '\r') { return Err("incomplete escape sequence"); }
    *pos += 1;
    match kind {
        'n' => Ok('\n'), 'r' => Ok('\r'), 't' => Ok('\t'), '0' => Ok('\0'),
        '\\' => Ok('\\'), '\'' => Ok('\''), '"' => Ok('"'),
        'x' => {
            let mut value = 0;
            for _ in 0..2 {
                let digit = source.get(*pos).filter(|c| c.is_ascii_hexdigit())
                    .and_then(|c| c.to_digit(16)).ok_or("hex escape requires exactly two hexadecimal digits")?;
                *pos += 1;
                value = value * 16 + digit;
            }
            if value > 0x7f {
                return Err("hex escape must be ASCII (00..7F); use a Unicode escape or a numeric byte buffer");
            }
            Ok(char::from_u32(value).unwrap())
        }
        'u' => {
            if source.get(*pos) != Some(&'{') { return Err("Unicode escape requires braces: \\u{...}"); }
            *pos += 1;
            let mut value = 0;
            let mut digits = 0;
            loop {
                match source.get(*pos) {
                    Some('}') => {
                        *pos += 1;
                        if digits == 0 { return Err("Unicode escape requires one to six hexadecimal digits"); }
                        return char::from_u32(value).ok_or("Unicode escape is not a valid Unicode scalar value");
                    }
                    Some(c) if c.is_ascii_hexdigit() => {
                        if digits == 6 { return Err("Unicode escape requires one to six hexadecimal digits"); }
                        value = value * 16 + c.to_digit(16).unwrap();
                        digits += 1;
                        *pos += 1;
                    }
                    _ => return Err("Unicode escape requires one to six hexadecimal digits and a closing brace"),
                }
            }
        }
        _ => Err("unknown escape sequence"),
    }
}

#[cfg(test)]
mod tests {
    use super::decode_escape;

    #[test]
    fn decode_exactly_one_escape() {
        for (input, expected, consumed) in [
            ("x1bF", '\u{1b}', 3), ("x007", '\0', 3), ("x7f0", '\u{7f}', 3),
            ("u{4e2d}next", '中', 7), ("u{1F680}", '🚀', 8),
            ("u{10ffff}", '\u{10ffff}', 9), ("u{000000}", '\0', 9),
        ] {
            let mut pos = 0;
            assert_eq!(decode_escape(&input.chars().collect::<Vec<_>>(), &mut pos), Ok(expected), "{input}");
            assert_eq!(pos, consumed, "{input}");
        }
    }

    #[test]
    fn malformed_escapes_do_not_become_characters() {
        for input in ["", "\n", "q", "X1b", "x", "x1", "xGG", "x80", "xff",
            "u1234", "u{}", "u{1", "u{_1}", "u{0000000}", "u{d800}", "u{DFFF}", "u{110000}"] {
            let chars: Vec<_> = input.chars().collect();
            let mut pos = 0;
            assert!(decode_escape(&chars, &mut pos).is_err(), "{input}");
            assert!(pos <= chars.len(), "{input}");
        }
    }
}
