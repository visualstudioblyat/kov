// syntax highlighting — Kov uses the real lexer, others get basic keyword highlighting

pub fn highlight(code: &str, lang: &str) -> String {
    match lang {
        "kov" => highlight_kov(code),
        "rust" => highlight_keywords(code, &RUST_KEYWORDS, &RUST_TYPES),
        "c" => highlight_keywords(code, &C_KEYWORDS, &C_TYPES),
        "js" | "javascript" => highlight_keywords(code, &JS_KEYWORDS, &[]),
        "bash" | "sh" => highlight_bash(code),
        "toml" => highlight_toml(code),
        "json" => highlight_json(code),
        _ => escape(code),
    }
}

fn highlight_kov(code: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            result.push_str("<span class=\"cm\">");
            while i < chars.len() && chars[i] != '\n' {
                push_escaped(&mut result, chars[i]);
                i += 1;
            }
            result.push_str("</span>");
            continue;
        }

        if c == '"' {
            result.push_str("<span class=\"s\">&quot;");
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                push_escaped(&mut result, chars[i]);
                i += 1;
            }
            if i < chars.len() { i += 1; }
            result.push_str("&quot;</span>");
            continue;
        }

        if c.is_ascii_digit() {
            result.push_str("<span class=\"n\">");
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == 'x') {
                result.push(chars[i]);
                i += 1;
            }
            result.push_str("</span>");
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "fn" | "let" | "mut" | "if" | "else" | "loop" | "while" | "for" | "in"
                | "match" | "return" | "break" | "continue" | "struct" | "enum" | "trait"
                | "impl" | "board" | "interrupt" | "extern" | "static" | "const" | "type"
                | "import" | "try" | "true" | "false" | "asm" => {
                    result.push_str(&format!("<span class=\"k\">{}</span>", word));
                }
                "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "bool"
                | "void" | "usize" | "isize" => {
                    result.push_str(&format!("<span class=\"t\">{}</span>", word));
                }
                _ => result.push_str(&word),
            }
            continue;
        }

        if c == '#' {
            result.push_str("<span class=\"a\">#");
            i += 1;
            if i < chars.len() && chars[i] == '[' {
                while i < chars.len() && chars[i] != ']' {
                    push_escaped(&mut result, chars[i]);
                    i += 1;
                }
                if i < chars.len() { result.push(']'); i += 1; }
            }
            result.push_str("</span>");
            continue;
        }

        push_escaped(&mut result, c);
        i += 1;
    }
    result
}

fn highlight_keywords(code: &str, keywords: &[&str], types: &[&str]) -> String {
    let mut result = String::new();
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            result.push_str("<span class=\"cm\">");
            while i < chars.len() && chars[i] != '\n' {
                push_escaped(&mut result, chars[i]);
                i += 1;
            }
            result.push_str("</span>");
            continue;
        }
        if c == '"' {
            result.push_str("<span class=\"s\">");
            push_escaped(&mut result, c);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                push_escaped(&mut result, chars[i]);
                i += 1;
            }
            if i < chars.len() { push_escaped(&mut result, chars[i]); i += 1; }
            result.push_str("</span>");
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
            let word: String = chars[start..i].iter().collect();
            if keywords.contains(&word.as_str()) {
                result.push_str(&format!("<span class=\"k\">{}</span>", word));
            } else if types.contains(&word.as_str()) {
                result.push_str(&format!("<span class=\"t\">{}</span>", word));
            } else {
                result.push_str(&word);
            }
            continue;
        }
        if c.is_ascii_digit() {
            result.push_str("<span class=\"n\">");
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                result.push(chars[i]);
                i += 1;
            }
            result.push_str("</span>");
            continue;
        }
        push_escaped(&mut result, c);
        i += 1;
    }
    result
}

fn highlight_bash(code: &str) -> String {
    let mut result = String::new();
    for line in code.lines() {
        if line.starts_with('#') {
            result.push_str(&format!("<span class=\"cm\">{}</span>\n", escape(line)));
        } else if line.starts_with('$') {
            result.push_str(&format!("<span class=\"k\">$</span>{}\n", escape(&line[1..])));
        } else {
            result.push_str(&escape(line));
            result.push('\n');
        }
    }
    result
}

fn highlight_toml(code: &str) -> String {
    let mut result = String::new();
    for line in code.lines() {
        if line.trim().starts_with('#') {
            result.push_str(&format!("<span class=\"cm\">{}</span>\n", escape(line)));
        } else if line.trim().starts_with('[') {
            result.push_str(&format!("<span class=\"k\">{}</span>\n", escape(line)));
        } else if let Some((key, val)) = line.split_once('=') {
            result.push_str(&format!("<span class=\"t\">{}</span>=<span class=\"s\">{}</span>\n", escape(key), escape(val)));
        } else {
            result.push_str(&escape(line));
            result.push('\n');
        }
    }
    result
}

fn highlight_json(code: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            result.push_str("<span class=\"s\">&quot;");
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                push_escaped(&mut result, chars[i]);
                i += 1;
            }
            if i < chars.len() { i += 1; }
            result.push_str("&quot;</span>");
        } else if chars[i].is_ascii_digit() || chars[i] == '-' {
            result.push_str("<span class=\"n\">");
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '-') {
                result.push(chars[i]);
                i += 1;
            }
            result.push_str("</span>");
        } else {
            push_escaped(&mut result, chars[i]);
            i += 1;
        }
    }
    result
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        _ => out.push(c),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

const RUST_KEYWORDS: [&str; 20] = [
    "fn", "let", "mut", "if", "else", "loop", "while", "for", "in", "match",
    "return", "break", "continue", "struct", "enum", "impl", "use", "pub",
    "mod", "trait",
];
const RUST_TYPES: [&str; 10] = [
    "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "bool", "String",
];
const C_KEYWORDS: [&str; 15] = [
    "if", "else", "while", "for", "return", "void", "struct", "typedef",
    "enum", "switch", "case", "break", "continue", "static", "const",
];
const C_TYPES: [&str; 8] = [
    "int", "char", "float", "double", "uint32_t", "uint8_t", "uint16_t", "size_t",
];
const JS_KEYWORDS: [&str; 15] = [
    "function", "const", "let", "var", "if", "else", "for", "while", "return",
    "class", "import", "export", "async", "await", "new",
];
