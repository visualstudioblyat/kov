use crate::lexer::token::Span;

pub fn format_error(source: &str, span: Span, message: &str) -> String {
    let (line_num, col, line_text) = locate(source, span.start);
    let width = ((span.end - span.start) as usize).max(1);
    let carets = "^".repeat(width.min(line_text.len().saturating_sub(col - 1)));

    format!(
        "error: {}\n  --> {}:{}\n   |\n{:>3}| {}\n   | {}{}\n",
        message,
        line_num,
        col,
        line_num,
        line_text,
        " ".repeat(col - 1),
        carets,
    )
}

fn locate(source: &str, byte_offset: u32) -> (usize, usize, &str) {
    let offset = byte_offset as usize;
    let mut line_num = 1;
    let mut line_start = 0;

    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line_num += 1;
            line_start = i + 1;
        }
    }

    let col = offset - line_start + 1;
    let line_end = source[line_start..]
        .find('\n')
        .map(|p| line_start + p)
        .unwrap_or(source.len());
    let line_text = source[line_start..line_end].trim_end();

    (line_num, col, line_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_to_correct_location() {
        let src = "let x = 1;\nlet y = true;\nlet z = x + y;\n";
        let output = format_error(src, Span::new(30, 35), "type mismatch");
        assert!(output.contains("type mismatch"));
        assert!(output.contains("3")); // line 3
    }

    #[test]
    fn first_line_error() {
        let src = "let x = bad;";
        let output = format_error(src, Span::new(8, 11), "undefined");
        assert!(output.contains("1:9")); // line 1, col 9
        assert!(output.contains("^^^"));
    }
}
