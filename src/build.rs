use crate::parser::ast::*;
use std::collections::HashMap;

pub struct BuildConfig {
    pub board: Option<String>,
    pub features: Vec<String>,
    pub release: bool,
}

impl BuildConfig {
    pub fn new() -> Self {
        Self {
            board: None,
            features: Vec::new(),
            release: false,
        }
    }

    pub fn from_args(args: &[String]) -> Self {
        let mut cfg = Self::new();
        for i in 0..args.len() {
            if args[i] == "--board" && i + 1 < args.len() {
                cfg.board = Some(args[i + 1].clone());
            }
            if args[i] == "--features" && i + 1 < args.len() {
                cfg.features = args[i + 1]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
            }
            if args[i] == "--release" {
                cfg.release = true;
            }
        }
        cfg
    }

    fn matches(&self, name: &str, value: &str) -> bool {
        match name {
            "board" => self.board.as_deref() == Some(value),
            "feature" => self.features.contains(&value.to_string()),
            "release" => self.release && value == "true",
            "debug" => !self.release && value == "true",
            _ => false,
        }
    }
}

// strip items that don't match #[cfg(...)] conditions
pub fn apply_cfg(program: &mut Program, config: &BuildConfig) {
    // extract board name from source if not set via CLI
    let board = config.board.clone().or_else(|| {
        program.items.iter().find_map(|item| {
            if let TopItem::Board(b) = item {
                Some(b.name.clone())
            } else {
                None
            }
        })
    });

    let cfg = BuildConfig {
        board,
        features: config.features.clone(),
        release: config.release,
    };

    program.items.retain(|item| {
        let attrs = match item {
            TopItem::Function(f) => &f.attrs,
            _ => return true,
        };

        for attr in attrs {
            if attr.name == "cfg" {
                if let Some(Expr::Binary(lhs, BinOp::Eq, rhs, _)) = attr.args.first() {
                    if let (Expr::Ident(name, _), Expr::StringLit(value, _)) =
                        (lhs.as_ref(), rhs.as_ref())
                    {
                        return cfg.matches(name, value);
                    }
                }
                if let Some(Expr::Ident(name, _)) = attr.args.first() {
                    // check features and board name
                    return cfg.features.contains(name)
                        || cfg.board.as_deref() == Some(name.as_str());
                }
                return false;
            }
        }
        true
    });
}

// parse kov.toml — minimal TOML parser for project config
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub board: Option<String>,
    pub features: Vec<String>,
}

impl ProjectConfig {
    pub fn from_toml(content: &str) -> Self {
        let mut name = "unnamed".to_string();
        let mut version = "0.1.0".to_string();
        let mut board = None;
        let mut features = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() || line.starts_with('[') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim().trim_matches('"');
                let val = val.trim().trim_matches('"');
                match key {
                    "name" => name = val.to_string(),
                    "version" => version = val.to_string(),
                    "board" => board = Some(val.to_string()),
                    "features" => {
                        features = val
                            .trim_matches(['[', ']'])
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    _ => {}
                }
            }
        }

        Self {
            name,
            version,
            board,
            features,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kov_toml() {
        let toml = r#"
[package]
name = "blink"
version = "0.1.0"
board = "esp32c3"
features = ["uart", "spi"]
"#;
        let config = ProjectConfig::from_toml(toml);
        assert_eq!(config.name, "blink");
        assert_eq!(config.version, "0.1.0");
        assert_eq!(config.board, Some("esp32c3".into()));
        assert_eq!(config.features, vec!["uart", "spi"]);
    }

    #[test]
    fn cfg_filters_by_board() {
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let tokens = Lexer::tokenize(
            r#"board esp32c3 { gpio: GPIO @ 0x6000_4000, clock: 160_000_000, }
               #[cfg(esp32c3)] fn esp_only() { }
               #[cfg(ch32v003)] fn ch32_only() { }
               fn always() { }"#,
        )
        .unwrap();
        let mut program = Parser::new(tokens).parse().unwrap();

        let config = BuildConfig {
            board: Some("esp32c3".into()),
            features: Vec::new(),
            release: false,
        };
        apply_cfg(&mut program, &config);

        let fn_names: Vec<String> = program
            .items
            .iter()
            .filter_map(|item| {
                if let TopItem::Function(f) = item {
                    Some(f.name.clone())
                } else {
                    None
                }
            })
            .collect();

        assert!(fn_names.contains(&"esp_only".into()));
        assert!(!fn_names.contains(&"ch32_only".into()));
        assert!(fn_names.contains(&"always".into()));
    }
}
