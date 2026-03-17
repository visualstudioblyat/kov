// minimal C header parser — extracts function declarations
// generates extern "C" fn declarations in Kov syntax

pub fn parse_header(content: &str) -> Vec<String> {
    let mut decls = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // skip preprocessor, comments, empty
        if line.starts_with('#') || line.starts_with("//") || line.is_empty() {
            continue;
        }
        // skip typedefs, structs for now
        if line.starts_with("typedef") || line.starts_with("struct") {
            continue;
        }
        // try to parse: return_type name(params);
        if let Some(decl) = try_parse_fn_decl(line) {
            decls.push(decl);
        }
    }

    decls
}

fn try_parse_fn_decl(line: &str) -> Option<String> {
    let line = line.trim_end_matches(';').trim();
    let paren = line.find('(')?;
    let before_paren = &line[..paren].trim();
    let params_str = &line[paren + 1..line.rfind(')')?];

    // split "void HAL_GPIO_Write" into return type + name
    let last_space = before_paren.rfind(' ')?;
    let ret_type = before_paren[..last_space].trim();
    let name = before_paren[last_space + 1..].trim();

    // skip if name starts with _ (internal)
    if name.starts_with("__") {
        return None;
    }

    // convert C types to Kov types
    let kov_ret = c_type_to_kov(ret_type);
    let kov_params = parse_c_params(params_str);

    let params_kov: Vec<String> = kov_params
        .iter()
        .enumerate()
        .map(|(i, ty)| format!("arg{}: {}", i, ty))
        .collect();

    let ret_str = if kov_ret == "void" {
        String::new()
    } else {
        format!(" {}", kov_ret)
    };

    Some(format!(
        "extern \"C\" fn {}({}){};",
        name,
        params_kov.join(", "),
        ret_str
    ))
}

fn parse_c_params(params: &str) -> Vec<String> {
    if params.trim() == "void" || params.trim().is_empty() {
        return Vec::new();
    }
    params
        .split(',')
        .map(|p| {
            let p = p.trim();
            // extract type (everything before the last word which is the param name)
            let parts: Vec<&str> = p.split_whitespace().collect();
            if parts.len() >= 2 {
                c_type_to_kov(&parts[..parts.len() - 1].join(" "))
            } else if parts.len() == 1 {
                c_type_to_kov(parts[0])
            } else {
                "u32".into()
            }
        })
        .collect()
}

fn c_type_to_kov(c_type: &str) -> String {
    let t = c_type
        .trim()
        .replace("const ", "")
        .replace("volatile ", "")
        .replace("unsigned ", "u")
        .replace("signed ", "i");
    match t.trim() {
        "void" => "void".into(),
        "int" | "iint" => "i32".into(),
        "uint" | "uint32_t" | "ulong" => "u32".into(),
        "uint8_t" | "uchar" => "u8".into(),
        "uint16_t" | "ushort" => "u16".into(),
        "uint64_t" => "u64".into(),
        "int8_t" | "ichar" | "char" => "i8".into(),
        "int16_t" | "ishort" | "short" => "i16".into(),
        "int32_t" | "ilong" | "long" => "i32".into(),
        "int64_t" => "i64".into(),
        "bool" | "_Bool" => "bool".into(),
        "float" | "double" => "u32".into(), // no float support yet
        s if s.contains('*') => "u32".into(), // pointers → u32 on rv32
        _ => "u32".into(),
    }
}

pub fn generate_kov(decls: &[String]) -> String {
    let mut out = String::from("// generated from C header\n\n");
    for d in decls {
        out.push_str(d);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_header() {
        let header = r#"
#include <stdint.h>

void HAL_GPIO_WritePin(uint32_t port, uint32_t pin, uint32_t state);
uint32_t HAL_GPIO_ReadPin(uint32_t port, uint32_t pin);
void HAL_Delay(uint32_t ms);
"#;
        let decls = parse_header(header);
        assert_eq!(decls.len(), 3);
        assert!(decls[0].contains("HAL_GPIO_WritePin"));
        assert!(decls[1].contains("HAL_GPIO_ReadPin"));
        assert!(decls[1].contains("u32")); // return type
        assert!(decls[2].contains("HAL_Delay"));
    }

    #[test]
    fn c_types_converted() {
        assert_eq!(c_type_to_kov("uint32_t"), "u32");
        assert_eq!(c_type_to_kov("int"), "i32");
        assert_eq!(c_type_to_kov("void"), "void");
        assert_eq!(c_type_to_kov("uint8_t"), "u8");
        assert_eq!(c_type_to_kov("char*"), "u32");
    }
}
