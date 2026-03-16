use crate::ir::{Function, Op};
use std::collections::{HashMap, HashSet};

pub struct StackResult {
    pub function: String,
    pub frame_size: u32,
    pub max_depth: u32, // including callees
    pub limit: Option<u32>,
    pub exceeded: bool,
    pub call_chain: Vec<String>,
}

// compute frame size for a function from its IR
fn frame_size(func: &Function) -> u32 {
    let mut size = 16u32; // minimum: RA + S0
    for block in &func.blocks {
        for inst in &block.insts {
            if let Op::StackAlloc(n) = inst.op {
                size += n;
            }
        }
    }
    // align to 16
    (size + 15) & !15
}

// extract direct callees from a function
fn callees(func: &Function) -> Vec<String> {
    let mut calls = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Op::Call(name, _) = &inst.op {
                if !calls.contains(name) {
                    calls.push(name.clone());
                }
            }
        }
    }
    calls
}

pub fn analyze(functions: &[Function], target: &str, limit: Option<u32>) -> StackResult {
    let frames: HashMap<String, u32> = functions
        .iter()
        .map(|f| (f.name.clone(), frame_size(f)))
        .collect();

    let call_graph: HashMap<String, Vec<String>> = functions
        .iter()
        .map(|f| (f.name.clone(), callees(f)))
        .collect();

    // DFS to find deepest call chain
    let mut visited = HashSet::new();
    let (max_depth, chain) = deepest_path(target, &frames, &call_graph, &mut visited);

    let exceeded = limit.map(|l| max_depth > l).unwrap_or(false);

    StackResult {
        function: target.to_string(),
        frame_size: frames.get(target).copied().unwrap_or(0),
        max_depth,
        limit,
        exceeded,
        call_chain: chain,
    }
}

fn deepest_path(
    name: &str,
    frames: &HashMap<String, u32>,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
) -> (u32, Vec<String>) {
    let my_frame = frames.get(name).copied().unwrap_or(0);

    if visited.contains(name) {
        // recursion detected
        return (u32::MAX, vec![format!("{}(recursive)", name)]);
    }

    visited.insert(name.to_string());

    let mut worst = 0u32;
    let mut worst_chain = vec![name.to_string()];

    if let Some(calls) = graph.get(name) {
        for callee in calls {
            if frames.contains_key(callee) {
                let (depth, chain) = deepest_path(callee, frames, graph, visited);
                if depth != u32::MAX && depth > worst {
                    worst = depth;
                    let mut c = vec![name.to_string()];
                    c.extend(chain);
                    worst_chain = c;
                }
            }
        }
    }

    visited.remove(name);
    (my_frame + worst, worst_chain)
}

pub fn format_report(results: &[StackResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&format!(
            "  {}(): {} bytes (frame: {})",
            r.function, r.max_depth, r.frame_size
        ));
        if let Some(limit) = r.limit {
            if r.exceeded {
                out.push_str(&format!(" — EXCEEDS limit of {}", limit));
            } else {
                out.push_str(&format!(" — within limit of {}", limit));
            }
        }
        out.push('\n');
        if r.call_chain.len() > 1 {
            out.push_str(&format!("    chain: {}\n", r.call_chain.join(" → ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower::Lowering;
    use crate::ir::opt;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn stack(src: &str) -> Vec<StackResult> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            opt::optimize(func);
        }
        ir.functions
            .iter()
            .map(|f| analyze(&ir.functions, &f.name, None))
            .collect()
    }

    #[test]
    fn single_function_stack() {
        let results = stack("fn f() { }");
        assert_eq!(results.len(), 1);
        assert!(results[0].frame_size >= 16);
    }

    #[test]
    fn call_chain_depth() {
        let results = stack("fn a() { b(); }\nfn b() { c(); }\nfn c() { }");
        let a = results.iter().find(|r| r.function == "a").unwrap();
        let c = results.iter().find(|r| r.function == "c").unwrap();
        assert!(
            a.max_depth > c.max_depth,
            "a calls b calls c, should be deeper"
        );
        assert!(a.call_chain.len() >= 3);
    }

    #[test]
    fn limit_exceeded() {
        let tokens = Lexer::tokenize("fn f() { }").unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);
        let result = analyze(&ir.functions, "f", Some(4));
        assert!(result.exceeded, "16-byte frame should exceed 4-byte limit");
    }
}
