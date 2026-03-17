use std::collections::HashMap;

pub struct Package {
    pub name: String,
    pub version: String,
    pub board: Option<String>,
    pub features: Vec<String>,
    pub deps: HashMap<String, DepSpec>,
}

pub struct DepSpec {
    pub git: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
}

impl Package {
    pub fn from_toml(content: &str) -> Self {
        let mut name = "unnamed".into();
        let mut version = "0.1.0".into();
        let mut board = None;
        let mut features = Vec::new();
        let mut deps: HashMap<String, DepSpec> = HashMap::new();

        let mut in_deps = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if line == "[package]" {
                in_deps = false;
                continue;
            }
            if line == "[dependencies]" {
                in_deps = true;
                continue;
            }
            if line.starts_with('[') {
                in_deps = false;
                continue;
            }

            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim().trim_matches('"');
                let val = val.trim().trim_matches('"');

                if in_deps {
                    // dep = "version" or dep = { git = "url" } or dep = { path = "..." }
                    if val.starts_with('{') {
                        let mut spec = DepSpec {
                            git: None,
                            version: None,
                            path: None,
                        };
                        let inner = val.trim_matches(['{', '}']);
                        for part in inner.split(',') {
                            if let Some((k, v)) = part.split_once('=') {
                                let k = k.trim().trim_matches('"');
                                let v = v.trim().trim_matches(['"', ' ']);
                                match k {
                                    "git" => spec.git = Some(v.into()),
                                    "version" => spec.version = Some(v.into()),
                                    "path" => spec.path = Some(v.into()),
                                    _ => {}
                                }
                            }
                        }
                        deps.insert(key.into(), spec);
                    } else {
                        deps.insert(
                            key.into(),
                            DepSpec {
                                git: None,
                                version: Some(val.into()),
                                path: None,
                            },
                        );
                    }
                } else {
                    match key {
                        "name" => name = val.into(),
                        "version" => version = val.into(),
                        "board" => board = Some(val.into()),
                        "features" => {
                            features = val
                                .trim_matches(['[', ']'])
                                .split(',')
                                .map(|s| s.trim().trim_matches('"').into())
                                .filter(|s: &String| !s.is_empty())
                                .collect();
                        }
                        _ => {}
                    }
                }
            }
        }

        Self {
            name,
            version,
            board,
            features,
            deps,
        }
    }

    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[package]\n");
        out.push_str(&format!("name = \"{}\"\n", self.name));
        out.push_str(&format!("version = \"{}\"\n", self.version));
        if let Some(ref board) = self.board {
            out.push_str(&format!("board = \"{}\"\n", board));
        }
        if !self.features.is_empty() {
            let feats: Vec<String> = self.features.iter().map(|f| format!("\"{}\"", f)).collect();
            out.push_str(&format!("features = [{}]\n", feats.join(", ")));
        }
        if !self.deps.is_empty() {
            out.push_str("\n[dependencies]\n");
            for (name, spec) in &self.deps {
                if let Some(ref git) = spec.git {
                    out.push_str(&format!("{} = {{ git = \"{}\" }}\n", name, git));
                } else if let Some(ref path) = spec.path {
                    out.push_str(&format!("{} = {{ path = \"{}\" }}\n", name, path));
                } else if let Some(ref ver) = spec.version {
                    out.push_str(&format!("{} = \"{}\"\n", name, ver));
                }
            }
        }
        out
    }

    pub fn init_template(name: &str, board: &str) -> String {
        format!(
            r#"[package]
name = "{}"
version = "0.1.0"
board = "{}"

[dependencies]
"#,
            name, board
        )
    }
}

pub fn init_project(name: &str, board: &str) -> Result<(), String> {
    let dir = std::path::Path::new(name);
    if dir.exists() {
        return Err(format!("directory '{}' already exists", name));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{e}"))?;
    std::fs::write(dir.join("kov.toml"), Package::init_template(name, board))
        .map_err(|e| format!("{e}"))?;

    let main_src = format!(
        r#"board {} {{
    gpio: GPIO @ 0x6000_4000,
    clock: 160_000_000,
}}

#[stack(512)]
fn main(b: &mut {}) {{
    let led = b.gpio.pin(2, .output);
    loop {{
        led.high();
        delay_ms(500);
        led.low();
        delay_ms(500);
    }}
}}
"#,
        board, board
    );
    std::fs::write(dir.join("main.kv"), main_src).map_err(|e| format!("{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kov_toml_with_deps() {
        let toml = r#"
[package]
name = "blink"
version = "0.1.0"
board = "esp32c3"

[dependencies]
esp32c3-hal = { git = "https://github.com/example/esp32c3-hal" }
utils = "0.2.0"
"#;
        let pkg = Package::from_toml(toml);
        assert_eq!(pkg.name, "blink");
        assert_eq!(pkg.deps.len(), 2);
        assert!(pkg.deps["esp32c3-hal"].git.is_some());
        assert_eq!(pkg.deps["utils"].version, Some("0.2.0".into()));
    }

    #[test]
    fn roundtrip_toml() {
        let mut pkg = Package::from_toml("[package]\nname = \"test\"\nversion = \"1.0.0\"\n");
        pkg.deps.insert(
            "foo".into(),
            DepSpec {
                git: Some("https://example.com/foo".into()),
                version: None,
                path: None,
            },
        );
        let toml = pkg.to_toml();
        assert!(toml.contains("name = \"test\""));
        assert!(toml.contains("[dependencies]"));
        assert!(toml.contains("foo"));
    }
}
