// full markdown parser — CommonMark + extensions (callouts, tabs, playground blocks)

use crate::highlight;

#[derive(Debug)]
pub enum Node {
    Heading(u8, String, String), // level, id, text
    Paragraph(String),
    CodeBlock(String, String, bool), // lang, code, is_playground
    List(Vec<ListItem>),
    OrderedList(Vec<ListItem>),
    Blockquote(String),
    HorizontalRule,
    Table(Vec<Vec<String>>),
    Callout(String, String), // kind (note/warning/tip/danger), content
    Html(String),
}

#[derive(Debug)]
pub struct ListItem {
    pub text: String,
    pub checked: Option<bool>,
}

pub struct Frontmatter {
    pub title: Option<String>,
    pub description: Option<String>,
    pub order: Option<u32>,
}

pub fn parse(md: &str) -> (Frontmatter, Vec<Node>) {
    let mut nodes = Vec::new();
    let mut frontmatter = Frontmatter {
        title: None,
        description: None,
        order: None,
    };

    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    // parse frontmatter (---  ... ---)
    if i < lines.len() && lines[i].trim() == "---" {
        i += 1;
        while i < lines.len() && lines[i].trim() != "---" {
            if let Some((key, val)) = lines[i].split_once(':') {
                let key = key.trim();
                let val = val.trim().trim_matches('"');
                match key {
                    "title" => frontmatter.title = Some(val.into()),
                    "description" => frontmatter.description = Some(val.into()),
                    "order" => frontmatter.order = val.parse().ok(),
                    _ => {}
                }
            }
            i += 1;
        }
        if i < lines.len() { i += 1; } // skip closing ---
    }

    while i < lines.len() {
        let line = lines[i];

        // blank line
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // callout: :::note / :::warning / :::tip / :::danger
        if line.trim().starts_with(":::") {
            let kind = line.trim()[3..].trim().to_string();
            if !kind.is_empty() {
                i += 1;
                let mut content = String::new();
                while i < lines.len() && lines[i].trim() != ":::" {
                    content.push_str(lines[i]);
                    content.push('\n');
                    i += 1;
                }
                if i < lines.len() { i += 1; } // skip closing :::
                nodes.push(Node::Callout(kind, content.trim().into()));
                continue;
            }
        }

        // code block
        if line.trim().starts_with("```") {
            let meta = line.trim()[3..].trim();
            let is_playground = meta.contains("playground");
            let lang = meta.split_whitespace().next().unwrap_or("text")
                .replace("playground", "").trim().to_string();
            let lang = if lang.is_empty() { "text".into() } else { lang };
            i += 1;
            let mut code = String::new();
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            if i < lines.len() { i += 1; }
            nodes.push(Node::CodeBlock(lang, code, is_playground));
            continue;
        }

        // headings
        if line.starts_with("# ") {
            let text = line[2..].trim();
            let id = slug(text);
            if frontmatter.title.is_none() {
                frontmatter.title = Some(text.into());
            }
            nodes.push(Node::Heading(1, id, text.into()));
            i += 1;
            continue;
        }
        if line.starts_with("## ") {
            let text = line[3..].trim();
            nodes.push(Node::Heading(2, slug(text), text.into()));
            i += 1;
            continue;
        }
        if line.starts_with("### ") {
            let text = line[4..].trim();
            nodes.push(Node::Heading(3, slug(text), text.into()));
            i += 1;
            continue;
        }
        if line.starts_with("#### ") {
            let text = line[5..].trim();
            nodes.push(Node::Heading(4, slug(text), text.into()));
            i += 1;
            continue;
        }

        // horizontal rule
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            nodes.push(Node::HorizontalRule);
            i += 1;
            continue;
        }

        // blockquote
        if line.starts_with("> ") {
            let mut content = String::new();
            while i < lines.len() && lines[i].starts_with("> ") {
                content.push_str(&lines[i][2..]);
                content.push('\n');
                i += 1;
            }
            nodes.push(Node::Blockquote(content.trim().into()));
            continue;
        }

        // unordered list
        if line.trim().starts_with("- ") || line.trim().starts_with("* ") {
            let mut items = Vec::new();
            while i < lines.len() && (lines[i].trim().starts_with("- ") || lines[i].trim().starts_with("* ")) {
                let text = lines[i].trim()[2..].trim();
                let (checked, text) = if text.starts_with("[ ] ") {
                    (Some(false), text[4..].to_string())
                } else if text.starts_with("[x] ") || text.starts_with("[X] ") {
                    (Some(true), text[4..].to_string())
                } else {
                    (None, text.to_string())
                };
                items.push(ListItem { text, checked });
                i += 1;
            }
            nodes.push(Node::List(items));
            continue;
        }

        // ordered list
        if line.trim().chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && line.contains(". ")
        {
            let mut items = Vec::new();
            while i < lines.len() {
                let trimmed = lines[i].trim();
                if let Some(dot_pos) = trimmed.find(". ") {
                    if trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                        items.push(ListItem {
                            text: trimmed[dot_pos + 2..].to_string(),
                            checked: None,
                        });
                        i += 1;
                        continue;
                    }
                }
                break;
            }
            if !items.is_empty() {
                nodes.push(Node::OrderedList(items));
                continue;
            }
        }

        // table
        if line.contains('|') && line.trim().starts_with('|') {
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].contains('|') {
                let row: Vec<String> = lines[i]
                    .split('|')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
                // skip separator row (| --- | --- |)
                if !row.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
                    rows.push(row);
                }
                i += 1;
            }
            if !rows.is_empty() {
                nodes.push(Node::Table(rows));
            }
            continue;
        }

        // paragraph (collect consecutive non-blank lines)
        let mut para = String::new();
        while i < lines.len() && !lines[i].trim().is_empty()
            && !lines[i].starts_with('#')
            && !lines[i].trim().starts_with("```")
            && !lines[i].trim().starts_with("- ")
            && !lines[i].trim().starts_with(":::")
            && !lines[i].trim().starts_with("> ")
        {
            if !para.is_empty() { para.push(' '); }
            para.push_str(lines[i].trim());
            i += 1;
        }
        if !para.is_empty() {
            nodes.push(Node::Paragraph(para));
        }
    }

    (frontmatter, nodes)
}

pub fn to_html(nodes: &[Node]) -> String {
    let mut html = String::new();
    for node in nodes {
        match node {
            Node::Heading(level, id, text) => {
                html.push_str(&format!(
                    "<h{} id=\"{}\">{}</h{}>\n",
                    level, id, inline(text), level
                ));
            }
            Node::Paragraph(text) => {
                html.push_str(&format!("<p>{}</p>\n", inline(text)));
            }
            Node::CodeBlock(lang, code, is_playground) => {
                let highlighted = highlight::highlight(code, lang);
                if *is_playground {
                    html.push_str(&format!(
                        "<div class=\"playground\" data-lang=\"{}\">\
                         <pre><code class=\"lang-{}\">{}</code></pre>\
                         <button class=\"try-btn\" onclick=\"openPlayground(this)\">Try it</button>\
                         </div>\n",
                        lang, lang, highlighted
                    ));
                } else {
                    html.push_str(&format!(
                        "<div class=\"code-block\">\
                         <div class=\"code-header\"><span class=\"code-lang\">{}</span>\
                         <button class=\"copy-btn\" onclick=\"copyCode(this)\">Copy</button></div>\
                         <pre><code class=\"lang-{}\">{}</code></pre></div>\n",
                        lang, lang, highlighted
                    ));
                }
            }
            Node::List(items) => {
                html.push_str("<ul>\n");
                for item in items {
                    if let Some(checked) = item.checked {
                        let check = if checked { "checked disabled" } else { "disabled" };
                        html.push_str(&format!(
                            "<li class=\"task\"><input type=\"checkbox\" {} /> {}</li>\n",
                            check, inline(&item.text)
                        ));
                    } else {
                        html.push_str(&format!("<li>{}</li>\n", inline(&item.text)));
                    }
                }
                html.push_str("</ul>\n");
            }
            Node::OrderedList(items) => {
                html.push_str("<ol>\n");
                for item in items {
                    html.push_str(&format!("<li>{}</li>\n", inline(&item.text)));
                }
                html.push_str("</ol>\n");
            }
            Node::Blockquote(text) => {
                html.push_str(&format!("<blockquote><p>{}</p></blockquote>\n", inline(text)));
            }
            Node::HorizontalRule => {
                html.push_str("<hr />\n");
            }
            Node::Table(rows) => {
                html.push_str("<div class=\"table-wrap\"><table>\n");
                for (i, row) in rows.iter().enumerate() {
                    if i == 0 {
                        html.push_str("<thead><tr>");
                        for cell in row {
                            html.push_str(&format!("<th>{}</th>", inline(cell)));
                        }
                        html.push_str("</tr></thead>\n<tbody>\n");
                    } else {
                        html.push_str("<tr>");
                        for cell in row {
                            html.push_str(&format!("<td>{}</td>", inline(cell)));
                        }
                        html.push_str("</tr>\n");
                    }
                }
                html.push_str("</tbody></table></div>\n");
            }
            Node::Callout(kind, content) => {
                let icon = match kind.as_str() {
                    "note" => "&#9432;",
                    "warning" => "&#9888;",
                    "tip" => "&#128161;",
                    "danger" => "&#9888;",
                    _ => "",
                };
                html.push_str(&format!(
                    "<div class=\"callout callout-{}\">\
                     <div class=\"callout-title\">{} {}</div>\
                     <div class=\"callout-body\"><p>{}</p></div></div>\n",
                    kind, icon, kind, inline(content)
                ));
            }
            Node::Html(raw) => {
                html.push_str(raw);
                html.push('\n');
            }
        }
    }
    html
}

pub fn extract_toc(nodes: &[Node]) -> Vec<(u8, String, String)> {
    nodes.iter().filter_map(|n| {
        if let Node::Heading(level, id, text) = n {
            if *level >= 2 && *level <= 3 {
                return Some((*level, id.clone(), text.clone()));
            }
        }
        None
    }).collect()
}

fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

fn inline(text: &str) -> String {
    let mut result = escape(text);

    // bold **text**
    while let Some(s) = result.find("**") {
        if let Some(e) = result[s + 2..].find("**") {
            let inner = result[s + 2..s + 2 + e].to_string();
            result = format!("{}<strong>{}</strong>{}", &result[..s], inner, &result[s + 4 + e..]);
        } else { break; }
    }

    // italic *text* (but not **)
    let mut i = 0;
    let chars: Vec<char> = result.chars().collect();
    let mut out = String::new();
    while i < chars.len() {
        if chars[i] == '*' && (i + 1 >= chars.len() || chars[i + 1] != '*') {
            if let Some(end) = result[i + 1..].find('*') {
                let inner = &result[i + 1..i + 1 + end];
                out.push_str(&format!("<em>{}</em>", inner));
                i = i + 2 + end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    result = out;

    // inline code `text`
    while let Some(s) = result.find('`') {
        if let Some(e) = result[s + 1..].find('`') {
            let inner = &result[s + 1..s + 1 + e].to_string();
            result = format!("{}<code>{}</code>{}", &result[..s], inner, &result[s + 2 + e..]);
        } else { break; }
    }

    // links [text](url)
    while let Some(bs) = result.find('[') {
        if let Some(be) = result[bs..].find("](") {
            let abs_be = bs + be;
            if let Some(pe) = result[abs_be + 2..].find(')') {
                let text = &result[bs + 1..abs_be].to_string();
                let url = &result[abs_be + 2..abs_be + 2 + pe].to_string();
                result = format!("{}<a href=\"{}\">{}</a>{}", &result[..bs], url, text, &result[abs_be + 3 + pe..]);
                continue;
            }
        }
        break;
    }

    result
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
