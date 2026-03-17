use std::path::Path;

pub struct Config {
    pub title: String,
    pub description: String,
    pub base_url: String,
    pub theme: String,
    pub playground: bool,
    pub search: bool,
    pub github: String,
    pub twitter: String,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let mut cfg = Self::default();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() || line.starts_with('[') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"');
                match key {
                    "title" => cfg.title = val.into(),
                    "description" => cfg.description = val.into(),
                    "base_url" => cfg.base_url = val.into(),
                    "theme" => cfg.theme = val.into(),
                    "playground" => cfg.playground = val == "true",
                    "search" => cfg.search = val == "true",
                    "github" => cfg.github = val.into(),
                    "twitter" => cfg.twitter = val.into(),
                    _ => {}
                }
            }
        }
        cfg
    }

    pub fn default() -> Self {
        Self {
            title: "Kov Documentation".into(),
            description: "Documentation for the Kov programming language".into(),
            base_url: "https://kov.dev/docs".into(),
            theme: "dark".into(),
            playground: true,
            search: true,
            github: "https://github.com/visualstudioblyat/kov".into(),
            twitter: "https://x.com/assemblyenjoyer".into(),
        }
    }
}
