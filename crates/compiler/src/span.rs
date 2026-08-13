//! Source spans for diagnostics.
//!
//! Compiler spans are **UTF-8 byte offsets** into the source buffer.
//! LSP positions are 0-based `(line, character)` where `character` is counted
//! in **UTF-16 code units** (LSP §3.17 Position).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }
}

/// 0-based line and character (LSP Position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// Half-open range of positions (LSP Range).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// Convert a UTF-8 byte offset to a 0-based LSP position.
///
/// `character` is the number of UTF-16 code units from the start of the line
/// (after the preceding `\n`, if any). Offsets past the end of `src` clamp to
/// EOF. Offsets that fall mid-codepoint floor to the nearest prior boundary.
pub fn byte_to_position(src: &str, byte: usize) -> Position {
    let byte = floor_char_boundary(src, byte.min(src.len()));
    let before = &src[..byte];
    let line = before.bytes().filter(|&b| b == b'\n').count() as u32;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_prefix = &src[line_start..byte];
    let character = line_prefix.chars().map(|c| c.len_utf16() as u32).sum();
    Position { line, character }
}

/// Map a byte `Span` to an LSP `Range` in `src`.
pub fn span_to_range(src: &str, span: Span) -> Range {
    Range {
        start: byte_to_position(src, span.start),
        end: byte_to_position(src, span.end),
    }
}

/// Map a 0-based LSP position to a UTF-8 byte offset (clamped to `src.len()`).
///
/// `character` is counted in UTF-16 code units (same as [`byte_to_position`]).
pub fn position_to_byte(src: &str, line: u32, character: u32) -> usize {
    let mut cur_line = 0u32;
    let mut line_start = 0usize;
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if cur_line == line {
            break;
        }
        if bytes[i] == b'\n' {
            cur_line += 1;
            line_start = i + 1;
        }
        i += 1;
    }
    if cur_line != line {
        return src.len();
    }
    // Walk UTF-16 units from line_start.
    let mut units = 0u32;
    let mut j = line_start;
    while j < src.len() && units < character {
        if src.as_bytes()[j] == b'\n' {
            break;
        }
        let ch = src[j..].chars().next().unwrap();
        units += ch.len_utf16() as u32;
        j += ch.len_utf8();
    }
    j.min(src.len())
}

/// True if the 0-based LSP position falls within `span` (half-open).
pub fn span_covers(src: &str, span: Span, line: u32, character: u32) -> bool {
    let byte = position_to_byte(src, line, character);
    // Empty spans (e.g. EOF errors) match only the exact start.
    if span.start == span.end {
        return byte == span.start;
    }
    byte >= span.start && byte < span.end
}

/// Largest `index ≤ at` that is a char boundary (or `0`).
fn floor_char_boundary(src: &str, at: usize) -> usize {
    if at >= src.len() {
        return src.len();
    }
    let mut i = at;
    while i > 0 && !src.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_start_of_file() {
        assert_eq!(
            byte_to_position("hello", 0),
            Position {
                line: 0,
                character: 0
            }
        );
    }

    #[test]
    fn position_multi_line_ascii() {
        let src = "ab\ncd\nef";
        // 'c' is byte 3
        assert_eq!(
            byte_to_position(src, 3),
            Position {
                line: 1,
                character: 0
            }
        );
        // 'e' is byte 6
        assert_eq!(
            byte_to_position(src, 6),
            Position {
                line: 2,
                character: 0
            }
        );
        // past 'f'
        assert_eq!(
            byte_to_position(src, 8),
            Position {
                line: 2,
                character: 2
            }
        );
    }

    #[test]
    fn span_to_range_multi_line() {
        let src = "fn main() {\n  if 3 {\n  }\n  0\n}\n";
        // Locate "3" — after "  if "
        let start = src.find('3').expect("3");
        let range = span_to_range(src, Span::new(start, start + 1));
        assert_eq!(
            range,
            Range {
                start: Position {
                    line: 1,
                    character: 5
                },
                end: Position {
                    line: 1,
                    character: 6
                },
            }
        );
    }

    #[test]
    fn utf16_character_counts_surrogate_pairs() {
        // U+1F600 😀 is one Unicode scalar, two UTF-16 code units, four UTF-8 bytes.
        let src = "a😀b";
        // byte offset of 'b' is 1 + 4 = 5
        let pos = byte_to_position(src, 5);
        assert_eq!(
            pos,
            Position {
                line: 0,
                character: 3
            }
        ); // 'a' = 1, emoji = 2
        let pos_end = byte_to_position(src, src.len());
        assert_eq!(
            pos_end,
            Position {
                line: 0,
                character: 4
            }
        );
    }

    #[test]
    fn clamp_past_end_and_mid_codepoint() {
        let src = "hi😀";
        // UTF-16: 'h'+'i'+😀 = 1+1+2 = 4
        let eof = byte_to_position(src, 999);
        assert_eq!(
            eof,
            Position {
                line: 0,
                character: 4
            }
        );
        // mid-emoji: floor to start of emoji (after "hi") → character 2
        let mid = byte_to_position(src, 3); // inside 😀
        assert_eq!(
            mid,
            Position {
                line: 0,
                character: 2
            }
        );
    }

    #[test]
    fn empty_source() {
        assert_eq!(
            byte_to_position("", 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(span_to_range("", Span::dummy()), Range::default());
    }

    #[test]
    fn position_to_byte_roundtrip_ascii() {
        let src = "ab\ncd";
        assert_eq!(position_to_byte(src, 0, 0), 0);
        assert_eq!(position_to_byte(src, 0, 2), 2);
        assert_eq!(position_to_byte(src, 1, 0), 3);
        assert_eq!(position_to_byte(src, 1, 1), 4);
        let span = Span::new(3, 5); // "cd"
        assert!(span_covers(src, span, 1, 0));
        assert!(span_covers(src, span, 1, 1));
        assert!(!span_covers(src, span, 1, 2));
        assert!(!span_covers(src, span, 0, 0));
    }
}
