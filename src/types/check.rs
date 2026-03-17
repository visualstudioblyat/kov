use super::{Ty, TypeError};
use crate::lexer::token::Span;
use crate::parser::ast::*;
use std::collections::{HashMap, HashSet};

pub struct TypeChecker {
    errors: Vec<TypeError>,
    pub warnings: Vec<TypeError>,
    board_peripherals: HashMap<String, Vec<String>>,
    claimed_peripherals: HashSet<String>,
    structs: HashMap<String, Vec<(String, Ty)>>,
    enums: HashMap<String, Vec<String>>,
    fn_sigs: HashMap<String, (Vec<Ty>, Ty)>,
    statics: HashMap<String, Ty>,
}

struct FnScope {
    vars: HashMap<String, Ty>,
    var_spans: HashMap<String, Span>, // where each var was declared
    used_vars: HashSet<String>,       // vars that were actually read
    ret_type: Ty,
    is_interrupt: bool,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            board_peripherals: HashMap::new(),
            claimed_peripherals: HashSet::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            fn_sigs: HashMap::new(),
            statics: HashMap::new(),
        }
    }

    pub fn check(mut self, program: &Program) -> Result<Vec<TypeError>, Vec<TypeError>> {
        // returns Ok(warnings) or Err(errors)
        // first pass: collect definitions
        for item in &program.items {
            match item {
                TopItem::Board(b) => self.register_board(b),
                TopItem::Struct(s) => self.register_struct(s),
                TopItem::Enum(e) => self.register_enum(e),
                TopItem::Function(f) => self.register_fn(f),
                TopItem::Static(s) => {
                    self.statics.insert(s.name.clone(), Ty::from_ast(&s.ty));
                }
                TopItem::ExternFn(e) => {
                    let params: Vec<Ty> = e.params.iter().map(|p| Ty::from_ast(&p.ty)).collect();
                    let ret = e.ret_type.as_ref().map(Ty::from_ast).unwrap_or(Ty::Void);
                    self.fn_sigs.insert(e.name.clone(), (params, ret));
                }
                _ => {}
            }
        }

        // second pass: check functions
        for item in &program.items {
            match item {
                TopItem::Function(f) => self.check_fn(f, false),
                TopItem::Interrupt(i) => self.check_interrupt(i),
                _ => {}
            }
        }

        if self.errors.is_empty() {
            Ok(self.warnings)
        } else {
            Err(self.errors)
        }
    }

    fn err(&mut self, span: Span, msg: String) {
        self.errors.push(TypeError { message: msg, span });
    }

    fn warn(&mut self, span: Span, msg: String) {
        self.warnings.push(TypeError { message: msg, span });
    }

    fn register_board(&mut self, board: &BoardDef) {
        let fields: Vec<String> = board.fields.iter().map(|f| f.name.clone()).collect();
        self.board_peripherals.insert(board.name.clone(), fields);
    }

    fn register_struct(&mut self, s: &StructDef) {
        let fields = s
            .fields
            .iter()
            .map(|f| (f.name.clone(), Ty::from_ast(&f.ty)))
            .collect();
        self.structs.insert(s.name.clone(), fields);
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let variants = e.variants.iter().map(|v| v.name.clone()).collect();
        self.enums.insert(e.name.clone(), variants);
    }

    fn register_fn(&mut self, f: &FnDef) {
        let params: Vec<Ty> = f.params.iter().map(|p| Ty::from_ast(&p.ty)).collect();
        let ret = f.ret_type.as_ref().map(Ty::from_ast).unwrap_or(Ty::Void);
        self.fn_sigs.insert(f.name.clone(), (params, ret));
    }

    fn check_interrupt(&mut self, i: &InterruptDef) {
        let mut scope = FnScope {
            vars: HashMap::new(),
            var_spans: HashMap::new(),
            used_vars: HashSet::new(),
            ret_type: Ty::Void,
            is_interrupt: true,
        };
        self.check_block(&i.body, &mut scope);
    }

    fn check_fn(&mut self, f: &FnDef, is_interrupt: bool) {
        let ret_type = f.ret_type.as_ref().map(Ty::from_ast).unwrap_or(Ty::Void);
        let mut scope = FnScope {
            vars: HashMap::new(),
            var_spans: HashMap::new(),
            used_vars: HashSet::new(),
            ret_type,
            is_interrupt,
        };

        for param in &f.params {
            let ty = Ty::from_ast(&param.ty);
            // detect board parameter: &mut BoardName
            if let Ty::MutRef(inner) = &ty {
                if let Ty::Named(name) = inner.as_ref() {
                    if self.board_peripherals.contains_key(name) {
                        scope
                            .vars
                            .insert(param.name.clone(), Ty::Board(name.clone()));
                        continue;
                    }
                }
            }
            scope.vars.insert(param.name.clone(), ty);
        }

        self.check_block(&f.body, &mut scope);

        // warn about unused variables (skip _ prefixed)
        for (name, span) in &scope.var_spans {
            if !scope.used_vars.contains(name) && !name.starts_with('_') {
                self.warn(*span, format!("unused variable: {}", name));
            }
        }
    }

    fn check_block(&mut self, block: &Block, scope: &mut FnScope) {
        for stmt in &block.stmts {
            self.check_stmt(stmt, scope);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt, scope: &mut FnScope) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                value,
                span,
                ..
            } => {
                let val_ty = self.check_expr(value, scope);

                if let Some(declared) = ty {
                    let declared_ty = Ty::from_ast(declared);
                    if val_ty != Ty::Unknown && declared_ty != Ty::Unknown && val_ty != declared_ty
                    {
                        self.err(
                            *span,
                            format!(
                                "type mismatch: declared {} as {:?} but value is {:?}",
                                name, declared_ty, val_ty
                            ),
                        );
                    }
                }

                // check peripheral ownership — if the value is a peripheral claim
                self.check_peripheral_claim(value, scope, *span);

                scope.vars.insert(name.clone(), val_ty);
                scope.var_spans.insert(name.clone(), *span);
            }

            Stmt::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.check_expr(target, scope);
                let val_ty = self.check_expr(value, scope);
                if target_ty != Ty::Unknown && val_ty != Ty::Unknown && target_ty != val_ty {
                    self.err(
                        *span,
                        format!("cannot assign {:?} to {:?}", val_ty, target_ty),
                    );
                }
            }

            Stmt::Return(Some(expr), span) => {
                let ty = self.check_expr(expr, scope);
                if ty != Ty::Unknown && scope.ret_type != Ty::Unknown && ty != scope.ret_type {
                    self.err(
                        *span,
                        format!(
                            "return type mismatch: expected {:?}, got {:?}",
                            scope.ret_type, ty
                        ),
                    );
                }
            }

            Stmt::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                let cond_ty = self.check_expr(condition, scope);
                if cond_ty != Ty::Bool && cond_ty != Ty::Unknown {
                    self.err(
                        *span,
                        format!("if condition must be bool, got {:?}", cond_ty),
                    );
                }
                self.check_block(then_block, scope);
                if let Some(eb) = else_block {
                    match eb {
                        ElseBranch::Else(block) => self.check_block(block, scope),
                        ElseBranch::ElseIf(stmt) => self.check_stmt(stmt, scope),
                    }
                }
            }

            Stmt::While {
                condition,
                body,
                span,
                label: _,
            } => {
                let cond_ty = self.check_expr(condition, scope);
                if cond_ty != Ty::Bool && cond_ty != Ty::Unknown {
                    self.err(
                        *span,
                        format!("while condition must be bool, got {:?}", cond_ty),
                    );
                }
                self.check_block(body, scope);
            }

            Stmt::Loop(_, body, _) => self.check_block(body, scope),

            Stmt::For {
                var,
                start,
                end,
                body,
                ..
            } => {
                let start_ty = self.check_expr(start, scope);
                let end_ty = self.check_expr(end, scope);
                if start_ty != Ty::Unknown && !start_ty.is_integer() {
                    self.err(start.span(), "for range start must be integer".into());
                }
                if end_ty != Ty::Unknown && !end_ty.is_integer() {
                    self.err(end.span(), "for range end must be integer".into());
                }
                scope.vars.insert(var.clone(), Ty::U32);
                self.check_block(body, scope);
            }

            Stmt::Match { expr, arms, span } => {
                let scrutinee_ty = self.check_expr(expr, scope);
                let mut has_wildcard = false;
                let mut covered_variants: HashSet<String> = HashSet::new();
                for arm in arms {
                    match &arm.pattern {
                        Pattern::Wildcard => has_wildcard = true,
                        Pattern::Ident(_) => has_wildcard = true,
                        Pattern::IntLit(_) => {
                            if scrutinee_ty != Ty::Unknown && !scrutinee_ty.is_integer() {
                                self.err(
                                    arm.span,
                                    format!(
                                        "integer pattern on non-integer type {:?}",
                                        scrutinee_ty
                                    ),
                                );
                            }
                        }
                        Pattern::Variant(name, _) => {
                            // check variant exists in the enum
                            if let Ty::Named(enum_name) = &scrutinee_ty {
                                if let Some(variants) = self.enums.get(enum_name) {
                                    if !variants.contains(name) {
                                        self.err(
                                            arm.span,
                                            format!(
                                                "unknown variant '{}' for enum {}",
                                                name, enum_name
                                            ),
                                        );
                                    } else {
                                        covered_variants.insert(name.clone());
                                    }
                                }
                            }
                        }
                    }
                    self.check_expr(&arm.body, scope);
                }
                // exhaustiveness: check all enum variants covered or has wildcard
                if !has_wildcard && !arms.is_empty() {
                    if let Ty::Named(enum_name) = &scrutinee_ty {
                        if let Some(variants) = self.enums.get(enum_name) {
                            let missing: Vec<&String> = variants
                                .iter()
                                .filter(|v| !covered_variants.contains(*v))
                                .collect();
                            if !missing.is_empty() {
                                self.err(
                                    *span,
                                    format!(
                                        "match not exhaustive: missing variants {}",
                                        missing
                                            .iter()
                                            .map(|v| v.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                );
                            }
                        } else {
                            self.err(
                                *span,
                                "match is not exhaustive: add a _ wildcard arm".into(),
                            );
                        }
                    } else {
                        self.err(
                            *span,
                            "match is not exhaustive: add a _ wildcard arm".into(),
                        );
                    }
                }
            }

            Stmt::Expr(expr) => {
                self.check_expr(expr, scope);
            }
            _ => {}
        }
    }

    // the core ownership check: b.gpio.pin(2, .output) claims gpio pin 2
    fn check_peripheral_claim(&mut self, expr: &Expr, scope: &FnScope, span: Span) {
        // detect pattern: <board_var>.<peripheral>.pin(<num>, ...)
        if let Expr::MethodCall(obj, method, args, _) = expr {
            if let Expr::Field(board_expr, peripheral_name, _) = obj.as_ref() {
                if let Expr::Ident(var_name, _) = board_expr.as_ref() {
                    if let Some(Ty::Board(_)) = scope.vars.get(var_name) {
                        if method == "pin" {
                            if let Some(Expr::IntLit(pin_num, _)) = args.first() {
                                let key = format!("{}.{}", peripheral_name, pin_num);
                                if !self.claimed_peripherals.insert(key.clone()) {
                                    self.err(span, format!(
                                        "peripheral {} pin {} already claimed — move semantics prevent double-claim",
                                        peripheral_name, pin_num
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, scope: &mut FnScope) -> Ty {
        match expr {
            Expr::IntLit(_, _) => Ty::U32,
            Expr::FloatLit(_, _) => Ty::Unknown, // no float support yet
            Expr::StringLit(_, _) => Ty::Ptr(Box::new(Ty::U8)),
            Expr::CharLit(_, _) => Ty::U8,
            Expr::BoolLit(_, _) => Ty::Bool,

            Expr::Ident(name, span) => {
                if let Some(ty) = scope.vars.get(name) {
                    scope.used_vars.insert(name.clone());
                    ty.clone()
                } else if let Some(ty) = self.statics.get(name) {
                    ty.clone()
                } else {
                    self.err(*span, format!("undefined variable: {}", name));
                    Ty::Unknown
                }
            }

            Expr::Binary(lhs, op, rhs, span) => {
                let lt = self.check_expr(lhs, scope);
                let rt = self.check_expr(rhs, scope);

                match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Rem
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr
                    | BinOp::WrapAdd
                    | BinOp::WrapSub
                    | BinOp::WrapMul => {
                        if lt != Ty::Unknown && !lt.is_numeric() {
                            self.err(*span, format!("arithmetic on non-numeric type {:?}", lt));
                        }
                        // no implicit promotion: u8 + u32 is an error
                        if lt != Ty::Unknown
                            && rt != Ty::Unknown
                            && lt != rt
                            && lt.is_integer()
                            && rt.is_integer()
                        {
                            self.err(
                                *span,
                                format!(
                                    "type mismatch in arithmetic: {:?} and {:?} (no implicit promotion)",
                                    lt, rt
                                ),
                            );
                        }
                        lt
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        if lt != Ty::Bool && lt != Ty::Unknown {
                            self.err(*span, format!("logical op requires bool, got {:?}", lt));
                        }
                        Ty::Bool
                    }
                    _ => lt,
                }
            }

            Expr::Unary(op, inner, _) => {
                let ty = self.check_expr(inner, scope);
                match op {
                    UnaryOp::Neg => ty,
                    UnaryOp::Not => Ty::Bool,
                    UnaryOp::BitNot => ty,
                    UnaryOp::Ref => Ty::Ref(Box::new(ty)),
                    UnaryOp::RefMut => Ty::MutRef(Box::new(ty)),
                    UnaryOp::Deref => match ty {
                        Ty::Ptr(inner) | Ty::Ref(inner) | Ty::MutRef(inner) => *inner,
                        _ => Ty::Unknown,
                    },
                }
            }

            Expr::Field(obj, field, _) => {
                let obj_ty = self.check_expr(obj, scope);
                // board.gpio → peripheral type
                if let Ty::Board(board_name) = &obj_ty {
                    if let Some(fields) = self.board_peripherals.get(board_name) {
                        if fields.contains(field) {
                            return Ty::Peripheral(field.clone());
                        }
                    }
                }
                // struct field lookup
                if let Ty::Named(name) = &obj_ty {
                    if let Some(fields) = self.structs.get(name) {
                        if let Some((_, ty)) = fields.iter().find(|(n, _)| n == field) {
                            return ty.clone();
                        }
                    }
                }
                Ty::Unknown
            }

            Expr::MethodCall(obj, _method, args, _) => {
                self.check_expr(obj, scope);
                for arg in args {
                    self.check_expr(arg, scope);
                }
                Ty::Unknown // method return types need full resolution
            }

            Expr::Call(callee, args, span) => {
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a, scope)).collect();

                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some((param_tys, ret_ty)) = self.fn_sigs.get(name).cloned() {
                        // check arg count
                        if arg_tys.len() != param_tys.len() {
                            self.err(
                                *span,
                                format!(
                                    "{}() expects {} args, got {}",
                                    name,
                                    param_tys.len(),
                                    arg_tys.len()
                                ),
                            );
                        }
                        return ret_ty;
                    }
                    // unknown function — might be external, don't error
                } else {
                    self.check_expr(callee, scope);
                }
                Ty::Unknown
            }

            Expr::Index(obj, idx, span) => {
                let obj_ty = self.check_expr(obj, scope);
                let idx_ty = self.check_expr(idx, scope);
                if idx_ty != Ty::Unknown && !idx_ty.is_integer() {
                    self.err(*span, "array index must be integer".into());
                }
                match obj_ty {
                    Ty::Array(inner, _) | Ty::Slice(inner) => *inner,
                    _ => Ty::Unknown,
                }
            }

            Expr::StructLit(name, fields, span) => {
                if let Some(def_fields) = self.structs.get(name).cloned() {
                    // check all fields provided
                    for (def_name, _) in &def_fields {
                        if !fields.iter().any(|(n, _)| n == def_name) {
                            self.err(
                                *span,
                                format!("missing field '{}' in struct {}", def_name, name),
                            );
                        }
                    }
                    // check no extra fields
                    for (field_name, expr) in fields {
                        self.check_expr(expr, scope);
                        if !def_fields.iter().any(|(n, _)| n == field_name) {
                            self.err(
                                *span,
                                format!("unknown field '{}' in struct {}", field_name, name),
                            );
                        }
                    }
                    Ty::Named(name.clone())
                } else {
                    self.err(*span, format!("unknown struct: {}", name));
                    Ty::Unknown
                }
            }

            Expr::DotEnum(_, _) => Ty::Unknown,
            Expr::ArrayLit(elems, _) => {
                if let Some(first) = elems.first() {
                    self.check_expr(first, scope)
                } else {
                    Ty::Unknown
                }
            }

            _ => Ty::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check(src: &str) -> Result<(), Vec<TypeError>> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        TypeChecker::new().check(&program).map(|_| ())
    }

    fn check_with_warnings(src: &str) -> (Result<(), Vec<TypeError>>, Vec<TypeError>) {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        match TypeChecker::new().check(&program) {
            Ok(warnings) => (Ok(()), warnings),
            Err(errors) => (Err(errors), vec![]),
        }
    }

    #[test]
    fn valid_simple_fn() {
        assert!(check("fn f() { let x: u32 = 42; }").is_ok());
    }

    #[test]
    fn type_mismatch_in_let() {
        let result = check("fn f() { let x: bool = 42; }");
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs[0].message.contains("type mismatch"));
    }

    #[test]
    fn if_requires_bool() {
        let result = check("fn f() { if 42 { } }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("bool"));
    }

    #[test]
    fn undefined_variable() {
        let result = check("fn f() { let x = y; }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("undefined"));
    }

    #[test]
    fn peripheral_double_claim_rejected() {
        let src = r#"
            board esp32c3 { gpio: GPIO @ 0x6000_4000, }
            fn main(b: &mut esp32c3) {
                let led1 = b.gpio.pin(2, .output);
                let led2 = b.gpio.pin(2, .output);
            }
        "#;
        let result = check(src);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs[0].message.contains("already claimed"));
    }

    #[test]
    fn peripheral_different_pins_ok() {
        let src = r#"
            board esp32c3 { gpio: GPIO @ 0x6000_4000, }
            fn main(b: &mut esp32c3) {
                let led1 = b.gpio.pin(2, .output);
                let led2 = b.gpio.pin(3, .output);
            }
        "#;
        assert!(check(src).is_ok());
    }

    #[test]
    fn for_range_requires_integer() {
        let result = check("fn f() { for i in true..false { } }");
        assert!(result.is_err());
    }

    #[test]
    fn arithmetic_on_bool_rejected() {
        let result = check("fn f() { let x = true + false; }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("non-numeric"));
    }

    #[test]
    fn wrong_arg_count() {
        let result = check("fn add(a: u32, b: u32) u32 { return a + b; }\nfn f() { add(1); }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("expects 2 args"));
    }

    #[test]
    fn fn_return_type_inferred() {
        // calling a known function should give back its return type
        assert!(check("fn get() u32 { return 42; }\nfn f() { let x: u32 = get(); }").is_ok());
    }

    #[test]
    fn match_not_exhaustive() {
        let result = check("fn f(x: u32) { match x { 0 => 1, 1 => 2, } }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("exhaustive"));
    }

    #[test]
    fn match_with_wildcard_ok() {
        assert!(check("fn f(x: u32) { match x { 0 => 1, _ => 2, } }").is_ok());
    }

    #[test]
    fn struct_missing_field() {
        let result = check("struct Point { x: u32, y: u32 }\nfn f() { let p = Point { x: 1 }; }");
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("missing field"));
    }

    #[test]
    fn struct_unknown_field() {
        let result = check(
            "struct Point { x: u32, y: u32 }\nfn f() { let p = Point { x: 1, y: 2, z: 3 }; }",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("unknown field"));
    }

    #[test]
    fn struct_valid() {
        assert!(
            check("struct Point { x: u32, y: u32 }\nfn f() { let p = Point { x: 1, y: 2 }; }")
                .is_ok()
        );
    }

    #[test]
    fn valid_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        if let Err(errors) = check(&source) {
            for e in &errors {
                eprintln!("  {}", e);
            }
            panic!("{} type errors", errors.len());
        }
    }

    #[test]
    fn local_type_inference() {
        // let x = 42 should infer u32, then x + 1 should work
        assert!(check("fn f() { let x = 42; let y = x + 1; }").is_ok());
    }

    #[test]
    fn inference_from_fn_call() {
        // get() returns u32, so x should be u32
        assert!(
            check("fn get() u32 { return 42; }\nfn f() { let x = get(); let y: u32 = x; }").is_ok()
        );
    }

    #[test]
    fn inference_mismatch_caught() {
        // get() returns u32, assigning to bool should fail
        let result = check("fn get() u32 { return 42; }\nfn f() { let x: bool = get(); }");
        assert!(result.is_err());
    }

    #[test]
    fn warns_unused_variable() {
        let (result, warnings) = check_with_warnings("fn f() { let x = 42; }");
        assert!(result.is_ok());
        assert!(!warnings.is_empty(), "should warn about unused x");
        assert!(warnings[0].message.contains("unused variable"));
    }

    #[test]
    fn no_warn_underscore_prefix() {
        let (result, warnings) = check_with_warnings("fn f() { let _x = 42; }");
        assert!(result.is_ok());
        assert!(warnings.is_empty(), "_ prefixed vars should not warn");
    }

    #[test]
    fn no_warn_used_variable() {
        let (result, warnings) = check_with_warnings("fn f() u32 { let x = 42; return x; }");
        assert!(result.is_ok());
        assert!(warnings.is_empty(), "used variable should not warn");
    }

    #[test]
    #[test]
    fn integer_promotion_rejected() {
        // use function params to get distinct types
        let result = check("fn f(a: u8, b: u32) { let c = a + b; }");
        assert!(result.is_err());
        assert!(
            result.unwrap_err()[0]
                .message
                .contains("no implicit promotion")
        );
    }

    #[test]
    fn same_type_arithmetic_ok() {
        assert!(check("fn f(a: u32, b: u32) u32 { return a + b; }").is_ok());
    }

    #[test]
    #[test]
    fn enum_exhaustive_match() {
        // variant patterns need parens syntax: Variant()
        assert!(
            check(
                "enum Color { Red, Green, Blue }
             fn f(c: Color) { match c { Red() => 1, Green() => 2, Blue() => 3, } }"
            )
            .is_ok()
        );
    }

    #[test]
    fn enum_missing_variant() {
        let result = check(
            "enum Color { Red, Green, Blue }
             fn f(c: Color) { match c { Red() => 1, Green() => 2, } }",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("missing variants"));
    }
}
