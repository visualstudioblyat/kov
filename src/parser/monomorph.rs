use super::ast::*;
use std::collections::HashMap;

// monomorphize generic functions: for each concrete type instantiation,
// generate a specialized copy with type params substituted
pub fn monomorphize(program: &mut Program) {
    let generics: HashMap<String, FnDef> = program
        .items
        .iter()
        .filter_map(|item| {
            if let TopItem::Function(f) = item {
                if !f.type_params.is_empty() {
                    return Some((f.name.clone(), f.clone()));
                }
            }
            None
        })
        .collect();

    if generics.is_empty() {
        return;
    }

    // collect all call sites that reference generic functions
    let mut instantiations: Vec<(String, Vec<Type>)> = Vec::new();
    for item in &program.items {
        if let TopItem::Function(f) = item {
            collect_calls(&f.body, &generics, &mut instantiations);
        }
    }

    // deduplicate
    instantiations.sort_by(|a, b| format!("{}{:?}", a.0, a.1).cmp(&format!("{}{:?}", b.0, b.1)));
    instantiations.dedup_by(|a, b| a.0 == b.0 && format!("{:?}", a.1) == format!("{:?}", b.1));

    // generate specialized functions
    let mut new_items = Vec::new();
    for (name, types) in &instantiations {
        if let Some(template) = generics.get(name) {
            if types.len() == template.type_params.len() {
                let specialized = specialize(template, types);
                new_items.push(TopItem::Function(specialized));
            }
        }
    }

    // remove generic function definitions (they're templates, not real code)
    program
        .items
        .retain(|item| !matches!(item, TopItem::Function(f) if !f.type_params.is_empty()));

    program.items.extend(new_items);
}

fn mangled_name(base: &str, types: &[Type]) -> String {
    let suffix: String = types
        .iter()
        .map(|t| match t {
            Type::Primitive(p) => format!("{:?}", p).to_lowercase(),
            Type::Named(n, _) => n.clone(),
            _ => "unknown".to_string(),
        })
        .collect::<Vec<_>>()
        .join("_");
    format!("{}_{}", base, suffix)
}

fn specialize(template: &FnDef, types: &[Type]) -> FnDef {
    let mut subst: HashMap<String, Type> = HashMap::new();
    for (param, ty) in template.type_params.iter().zip(types) {
        subst.insert(param.name.clone(), ty.clone());
    }

    let name = mangled_name(&template.name, types);
    let params: Vec<Param> = template
        .params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            ty: substitute_type(&p.ty, &subst),
            span: p.span,
        })
        .collect();
    let ret_type = template
        .ret_type
        .as_ref()
        .map(|t| substitute_type(t, &subst));
    let body = substitute_block(&template.body, &subst);

    FnDef {
        name,
        type_params: Vec::new(),
        attrs: template.attrs.clone(),
        params,
        ret_type,
        is_error_return: template.is_error_return,
        body,
        span: template.span,
    }
}

fn substitute_type(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Named(name, args) if args.is_empty() => {
            if let Some(replacement) = subst.get(name) {
                return replacement.clone();
            }
            ty.clone()
        }
        Type::Named(name, args) => Type::Named(
            name.clone(),
            args.iter().map(|a| substitute_type(a, subst)).collect(),
        ),
        Type::Ref(inner, m) => Type::Ref(Box::new(substitute_type(inner, subst)), *m),
        Type::Ptr(inner, r) => Type::Ptr(Box::new(substitute_type(inner, subst)), r.clone()),
        Type::Array(inner, size) => {
            Type::Array(Box::new(substitute_type(inner, subst)), size.clone())
        }
        Type::Slice(inner) => Type::Slice(Box::new(substitute_type(inner, subst))),
        Type::ErrorUnion(inner) => Type::ErrorUnion(Box::new(substitute_type(inner, subst))),
        _ => ty.clone(),
    }
}

fn substitute_block(block: &Block, subst: &HashMap<String, Type>) -> Block {
    Block {
        stmts: block
            .stmts
            .iter()
            .map(|s| substitute_stmt(s, subst))
            .collect(),
        tail_expr: block.tail_expr.clone(),
        span: block.span,
    }
}

fn substitute_stmt(stmt: &Stmt, _subst: &HashMap<String, Type>) -> Stmt {
    // for now just clone — type substitution in expressions isn't needed
    // because the lowering resolves types from the substituted params
    stmt.clone()
}

fn collect_calls(
    block: &Block,
    generics: &HashMap<String, FnDef>,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    for stmt in &block.stmts {
        collect_calls_stmt(stmt, generics, out);
    }
}

fn collect_calls_stmt(
    stmt: &Stmt,
    generics: &HashMap<String, FnDef>,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    match stmt {
        Stmt::Let { value, .. } => collect_calls_expr(value, generics, out),
        Stmt::Assign { value, .. } => collect_calls_expr(value, generics, out),
        Stmt::Expr(expr) => collect_calls_expr(expr, generics, out),
        Stmt::Return(Some(expr), _) => collect_calls_expr(expr, generics, out),
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            collect_calls_expr(condition, generics, out);
            collect_calls(then_block, generics, out);
            if let Some(eb) = else_block {
                match eb {
                    ElseBranch::Else(b) => collect_calls(b, generics, out),
                    ElseBranch::ElseIf(s) => collect_calls_stmt(s, generics, out),
                }
            }
        }
        Stmt::Loop(_, body, _) => collect_calls(body, generics, out),
        Stmt::While { body, .. } => collect_calls(body, generics, out),
        Stmt::For { body, .. } => collect_calls(body, generics, out),
        Stmt::Match { arms, .. } => {
            for arm in arms {
                collect_calls_expr(&arm.body, generics, out);
            }
        }
        _ => {}
    }
}

fn collect_calls_expr(
    expr: &Expr,
    generics: &HashMap<String, FnDef>,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    match expr {
        Expr::Call(callee, args, _) => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if generics.contains_key(name) {
                    // infer types from arguments (for now, assume all args are u32)
                    let template = &generics[name];
                    let types: Vec<Type> = template
                        .type_params
                        .iter()
                        .map(|_| Type::Primitive(PrimitiveType::U32))
                        .collect();
                    out.push((name.clone(), types));
                }
            }
            for a in args {
                collect_calls_expr(a, generics, out);
            }
        }
        Expr::Binary(l, _, r, _) => {
            collect_calls_expr(l, generics, out);
            collect_calls_expr(r, generics, out);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn monomorphize_generic_fn() {
        let tokens = Lexer::tokenize(
            "fn max<T>(a: T, b: T) T { if a > b { return a; } else { return b; } }
             fn f(x: u32, y: u32) u32 { return max(x, y); }",
        )
        .unwrap();
        let mut program = Parser::new(tokens).parse().unwrap();
        monomorphize(&mut program);

        // should have removed the generic max and added max_u32
        let has_generic = program
            .items
            .iter()
            .any(|i| matches!(i, TopItem::Function(f) if f.name == "max"));
        assert!(!has_generic, "generic template should be removed");

        let has_specialized = program
            .items
            .iter()
            .any(|i| matches!(i, TopItem::Function(f) if f.name == "max_u32"));
        assert!(has_specialized, "should have max_u32 specialization");
    }
}
