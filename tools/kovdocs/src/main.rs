mod config;
mod highlight;
mod markdown;

use config::Config;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("kovdocs — static site generator for Kov documentation");
        eprintln!("usage: kovdocs <content_dir> <output_dir>");
        std::process::exit(1);
    }

    let content_dir = Path::new(&args[1]);
    let output_dir = Path::new(&args[2]);
    let config_path = content_dir.join("../kovdocs.toml");
    let cfg = Config::load(&config_path);

    fs::create_dir_all(output_dir).unwrap();
    fs::create_dir_all(output_dir.join("assets")).unwrap();

    let mut pages: Vec<Page> = Vec::new();

    // collect all .md files recursively
    collect_pages(content_dir, content_dir, &mut pages);
    pages.sort_by(|a, b| a.order.cmp(&b.order).then(a.slug.cmp(&b.slug)));

    let start = std::time::Instant::now();

    // build search index
    let mut search_entries = Vec::new();

    for page in &pages {
        search_entries.push(format!(
            "{{\"title\":\"{}\",\"url\":\"{}.html\",\"body\":\"{}\"}}",
            page.title.replace('"', "\\\""),
            page.slug,
            page.text_content.replace('"', "\\\"").replace('\n', " ")[..page.text_content.len().min(200)].to_string()
        ));
    }

    // generate each page
    for (i, page) in pages.iter().enumerate() {
        let nav = build_nav(&pages, &page.slug);
        let toc = build_toc(&page.toc);
        let prev = if i > 0 { Some(&pages[i - 1]) } else { None };
        let next = if i + 1 < pages.len() { Some(&pages[i + 1]) } else { None };
        let prev_next = build_prev_next(prev, next);
        let breadcrumb = format!("<div class=\"breadcrumb\"><a href=\"index.html\">Docs</a> / {}</div>", page.title);

        let html = template(&cfg, &page.title, &breadcrumb, &nav, &toc, &page.html, &prev_next);
        let out_path = output_dir.join(format!("{}.html", page.slug));
        fs::write(&out_path, &html).unwrap();
    }

    // index page
    let nav = build_nav(&pages, "index");
    let index_body = format!(
        "<h1>{}</h1><p>{}</p><ul>{}</ul>",
        cfg.title,
        cfg.description,
        pages.iter().map(|p| format!("<li><a href=\"{}.html\">{}</a></li>", p.slug, p.title)).collect::<Vec<_>>().join("\n")
    );
    let index = template(&cfg, &cfg.title, "", &nav, "", &index_body, "");
    fs::write(output_dir.join("index.html"), &index).unwrap();

    // assets
    fs::write(output_dir.join("assets/style.css"), CSS).unwrap();
    fs::write(output_dir.join("assets/app.js"), JS).unwrap();

    // search index
    let search_json = format!("[{}]", search_entries.join(","));
    fs::write(output_dir.join("assets/search-index.json"), &search_json).unwrap();

    let elapsed = start.elapsed();
    eprintln!("  kovdocs: {} pages in {:.0}ms", pages.len() + 1, elapsed.as_secs_f64() * 1000.0);
}

struct Page {
    slug: String,
    title: String,
    html: String,
    toc: Vec<(u8, String, String)>,
    text_content: String,
    order: u32,
    source: PathBuf,
}

fn collect_pages(dir: &Path, base: &Path, pages: &mut Vec<Page>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_pages(&path, base, pages);
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            let content = fs::read_to_string(&path).unwrap();
            let (fm, nodes) = markdown::parse(&content);
            let html = markdown::to_html(&nodes);
            let toc = markdown::extract_toc(&nodes);
            let title = fm.title.unwrap_or_else(|| {
                path.file_stem().unwrap().to_str().unwrap().to_string()
            });
            let slug = path.strip_prefix(base).unwrap()
                .with_extension("")
                .to_str().unwrap()
                .replace('\\', "/")
                .replace('/', "-");
            let text_content = strip_html(&html);
            let order = fm.order.unwrap_or(999);

            pages.push(Page { slug, title, html, toc, text_content, order, source: path });
        }
    }
}

fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' { in_tag = true; continue; }
        if c == '>' { in_tag = false; continue; }
        if !in_tag { out.push(c); }
    }
    out
}

fn build_nav(pages: &[Page], current: &str) -> String {
    let mut nav = String::from("<nav class=\"sidebar-nav\">\n<ul>\n");
    for page in pages {
        let active = if page.slug == current { " class=\"active\"" } else { "" };
        nav.push_str(&format!("<li{}><a href=\"{}.html\">{}</a></li>\n", active, page.slug, page.title));
    }
    nav.push_str("</ul>\n</nav>\n");
    nav
}

fn build_toc(toc: &[(u8, String, String)]) -> String {
    if toc.is_empty() { return String::new(); }
    let mut html = String::from("<div class=\"toc\"><div class=\"toc-title\">On this page</div>\n<ul>\n");
    for (level, id, text) in toc {
        let indent = if *level == 3 { " class=\"toc-sub\"" } else { "" };
        html.push_str(&format!("<li{}><a href=\"#{}\">{}</a></li>\n", indent, id, text));
    }
    html.push_str("</ul></div>\n");
    html
}

fn build_prev_next(prev: Option<&Page>, next: Option<&Page>) -> String {
    let mut html = String::from("<div class=\"prev-next\">");
    if let Some(p) = prev {
        html.push_str(&format!("<a href=\"{}.html\" class=\"prev\">← {}</a>", p.slug, p.title));
    } else {
        html.push_str("<span></span>");
    }
    if let Some(n) = next {
        html.push_str(&format!("<a href=\"{}.html\" class=\"next\">{} →</a>", n.slug, n.title));
    }
    html.push_str("</div>");
    html
}

fn template(cfg: &Config, title: &str, breadcrumb: &str, nav: &str, toc: &str, content: &str, prev_next: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — {site_title}</title>
<meta name="description" content="{desc}">
<meta property="og:title" content="{title} — {site_title}">
<meta property="og:description" content="{desc}">
<link rel="stylesheet" href="assets/style.css">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
</head>
<body>
<div class="top-bar">
<a href="index.html" class="logo">{site_title}</a>
<div class="top-actions">
<button class="search-btn" onclick="toggleSearch()">Search <kbd>Ctrl+K</kbd></button>
<a href="{github}" class="gh-link">GitHub</a>
</div>
</div>
<div class="search-overlay" id="search-overlay" style="display:none">
<div class="search-modal">
<input type="text" id="search-input" placeholder="Search docs..." oninput="doSearch(this.value)" autofocus>
<div id="search-results"></div>
</div>
</div>
<div class="layout">
<aside>{nav}</aside>
<main>
{breadcrumb}
<article>{content}</article>
{prev_next}
</main>
<div class="toc-col">{toc}</div>
</div>
<footer><p>generated by <a href="https://github.com/visualstudioblyat/kov">kovdocs</a></p></footer>
<script src="assets/app.js"></script>
</body>
</html>"#,
        title = title,
        site_title = cfg.title,
        desc = cfg.description,
        github = cfg.github,
        nav = nav,
        breadcrumb = breadcrumb,
        content = content,
        toc = toc,
        prev_next = prev_next,
    )
}

const CSS: &str = include_str!("style.css");
const JS: &str = include_str!("app.js");
