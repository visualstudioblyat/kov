use crate::{errors, lexer, parser, types};
use std::io::{self, BufRead, Write};
use std::sync::Mutex;

static LAST_TEXT: Mutex<Option<String>> = Mutex::new(None);

pub fn run_lsp() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        match read_message(&mut reader) {
            Ok(msg) => {
                if let Some(response) = handle_message(&msg) {
                    send_message(&mut writer, &response);
                }
            }
            Err(_) => break,
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<String, io::Error> {
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(len) = line.strip_prefix("Content-Length: ") {
            content_length = len.parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    io::Read::read_exact(reader, &mut body)?;
    Ok(String::from_utf8_lossy(&body).to_string())
}

fn send_message(writer: &mut impl Write, msg: &str) {
    let _ = write!(writer, "Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    let _ = writer.flush();
}

fn handle_message(msg: &str) -> Option<String> {
    let id = extract_json_int(msg, "\"id\":");
    let method = extract_json_str(msg, "\"method\":");

    match method.as_deref() {
        Some("initialize") => Some(format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{{"capabilities":{{"textDocumentSync":1,"hoverProvider":true,"completionProvider":{{"triggerCharacters":["."]}},"diagnosticProvider":{{"interFileDependencies":false,"workspaceDiagnostics":false}}}}}}}}"#,
            id.unwrap_or(0)
        )),
        Some("initialized") => None,
        Some("shutdown") => Some(format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":null}}"#,
            id.unwrap_or(0)
        )),
        Some("exit") => std::process::exit(0),
        Some("textDocument/didOpen") | Some("textDocument/didChange") => {
            let uri = extract_json_str(msg, "\"uri\":").unwrap_or_default();
            let text = extract_json_str(msg, "\"text\":").unwrap_or_default();
            if let Ok(mut last) = LAST_TEXT.lock() {
                *last = Some(text.clone());
            }
            let diagnostics = check_source(&text);
            let notification = format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":[{}]}}}}"#,
                uri,
                diagnostics.join(",")
            );
            Some(notification)
        }
        Some("textDocument/hover") => {
            let text = extract_json_str(msg, "\"text\":")
                .or_else(|| LAST_TEXT.lock().ok().and_then(|t| t.clone()))
                .unwrap_or_default();
            let line = extract_json_int(msg, "\"line\":").unwrap_or(0) as usize;
            let ch = extract_json_int(msg, "\"character\":").unwrap_or(0) as usize;
            let word = word_at_position(&text, line, ch);
            let info = hover_info(&word, &text);
            Some(format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":{{"contents":"{}"}}}}"#,
                id.unwrap_or(0),
                escape_json(&info)
            ))
        }
        Some("textDocument/completion") => {
            let items: Vec<String> = KEYWORDS
                .iter()
                .map(|kw| format!(r#"{{"label":"{}","kind":14}}"#, kw))
                .collect();
            Some(format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":[{}]}}"#,
                id.unwrap_or(0),
                items.join(",")
            ))
        }
        _ => None,
    }
}

fn check_source(source: &str) -> Vec<String> {
    let mut diags = Vec::new();

    let tokens = match lexer::Lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            diags.push(format!(
                r#"{{"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":1}}}},"severity":1,"message":"{}"}}"#,
                escape_json(&format!("{e}"))
            ));
            return diags;
        }
    };

    let program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(errors) => {
            for e in &errors {
                let (line, col, _) = errors::locate(source, e.span.start);
                let (eline, ecol, _) = errors::locate(source, e.span.end);
                diags.push(format!(
                    r#"{{"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"severity":1,"message":"{}"}}"#,
                    line.saturating_sub(1), col.saturating_sub(1),
                    eline.saturating_sub(1), ecol.saturating_sub(1),
                    escape_json(&e.message)
                ));
            }
            return diags;
        }
    };

    match types::check::TypeChecker::new().check(&program) {
        Ok(warnings) => {
            for w in &warnings {
                let (line, col, _) = errors::locate(source, w.span.start);
                let (eline, ecol, _) = errors::locate(source, w.span.end);
                diags.push(format!(
                    r#"{{"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"severity":2,"message":"{}"}}"#,
                    line.saturating_sub(1), col.saturating_sub(1),
                    eline.saturating_sub(1), ecol.saturating_sub(1),
                    escape_json(&w.message)
                ));
            }
        }
        Err(errors) => {
            for e in &errors {
                let (line, col, _) = errors::locate(source, e.span.start);
                let (eline, ecol, _) = errors::locate(source, e.span.end);
                diags.push(format!(
                    r#"{{"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"severity":1,"message":"{}"}}"#,
                    line.saturating_sub(1), col.saturating_sub(1),
                    eline.saturating_sub(1), ecol.saturating_sub(1),
                    escape_json(&e.message)
                ));
            }
        }
    }

    diags
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let start = json.find(key)? + key.len();
    let rest = json[start..].trim();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        None
    }
}

fn extract_json_int(json: &str, key: &str) -> Option<i64> {
    let start = json.find(key)? + key.len();
    let rest = json[start..].trim();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn word_at_position(text: &str, line: usize, ch: usize) -> String {
    let target_line = text.lines().nth(line).unwrap_or("");
    if ch >= target_line.len() {
        return String::new();
    }
    let bytes = target_line.as_bytes();
    let mut start = ch;
    let mut end = ch;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    target_line[start..end].to_string()
}

fn hover_info(word: &str, source: &str) -> String {
    // check keywords
    match word {
        "fn" => return "keyword: function declaration".into(),
        "let" => return "keyword: variable binding".into(),
        "mut" => return "keyword: mutable variable".into(),
        "if" | "else" => return "keyword: conditional".into(),
        "loop" | "while" | "for" => return "keyword: loop".into(),
        "match" => return "keyword: pattern matching".into(),
        "return" => return "keyword: return from function".into(),
        "break" => return "keyword: exit loop".into(),
        "continue" => return "keyword: skip to next iteration".into(),
        "struct" => return "keyword: struct definition".into(),
        "enum" => return "keyword: enum definition".into(),
        "trait" => return "keyword: trait definition".into(),
        "impl" => return "keyword: implementation block".into(),
        "board" => return "keyword: board hardware definition".into(),
        "try" => return "keyword: unwrap error union or propagate".into(),
        "static" => return "keyword: static/global variable".into(),
        "extern" => return "keyword: foreign function interface".into(),
        _ => {}
    }
    // check types
    match word {
        "u8" | "u16" | "u32" | "u64" => {
            return format!("type: unsigned {}-bit integer", &word[1..]);
        }
        "i8" | "i16" | "i32" | "i64" => return format!("type: signed {}-bit integer", &word[1..]),
        "bool" => return "type: boolean (true/false)".into(),
        "void" => return "type: no value".into(),
        _ => {}
    }
    // try to find function signature in source
    if let Ok(tokens) = lexer::Lexer::tokenize(source) {
        if let Ok(program) = parser::Parser::new(tokens).parse() {
            for item in &program.items {
                if let parser::ast::TopItem::Function(f) = item {
                    if f.name == word {
                        let params: Vec<String> = f
                            .params
                            .iter()
                            .map(|p| format!("{}: {:?}", p.name, p.ty))
                            .collect();
                        let ret = f
                            .ret_type
                            .as_ref()
                            .map(|t| format!(" -> {:?}", t))
                            .unwrap_or_default();
                        return format!("fn {}({}){}", f.name, params.join(", "), ret);
                    }
                }
            }
        }
    }
    word.to_string()
}

const KEYWORDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "impl",
    "board",
    "interrupt",
    "let",
    "mut",
    "if",
    "else",
    "loop",
    "while",
    "for",
    "in",
    "match",
    "break",
    "continue",
    "return",
    "static",
    "const",
    "type",
    "import",
    "extern",
    "try",
    "true",
    "false",
];
