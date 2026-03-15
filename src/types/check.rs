use std::collections::{HashMap, HashSet};
use crate::parser::ast::*;
use crate::lexer::token::Span;
use super::{Ty, TypeError};

pub struct TypeChecker {
    errors: Vec<TypeError>,
    // all known board peripheral names (from board{} definitions)
    board_peripherals: HashMap<String, Vec<String>>,
    // which peripherals have been claimed — the ownership enforcement
    claimed_peripherals: HashSet<String>,
    // struct definitions
    structs: HashMap<String, Vec<(String, Ty)>>,
    // enum definitions
    enums: HashMap<String, Vec<String>>,
}

struct FnScope {
    vars: HashMap<String, Ty>,
    ret_type: Ty,
    is_interrupt: bool,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            board_peripherals: HashMap::new(),
            claimed_peripherals: HashSet::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
        }
    }

    pub fn check(mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        // first pass: collect definitions
        for item in &program.items {
            match item {
                TopItem::Board(b) => self.register_board(b),
                TopItem::Struct(s) => self.register_struct(s),
                TopItem::Enum(e) => self.register_enum(e),
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

        if self.errors.is_empty() { Ok(()) } else { Err(self.errors) }
    }

    fn err(&mut self, span: Span, msg: String) {
        self.errors.push(TypeError { message: msg, span });
    }

    fn register_board(&mut self, board: &BoardDef) {
        let fields: Vec<String> = board.fields.iter().map(|f| f.name.clone()).collect();
        self.board_peripherals.insert(board.name.clone(), fields);
    }

    fn register_struct(&mut self, s: &StructDef) {
        let fields = s.fields.iter().map(|f| (f.name.clone(), Ty::from_ast(&f.ty))).collect();
        self.structs.insert(s.name.clone(), fields);
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let variants = e.variants.iter().map(|v| v.name.clone()).collect();
        self.enums.insert(e.name.clone(), variants);
    }

    fn check_interrupt(&mut self, i: &InterruptDef) {
        let mut scope = FnScope {
            vars: HashMap::new(),
            ret_type: Ty::Void,
            is_interrupt: true,
        };
        self.check_block(&i.body, &mut scope);
    }

    fn check_fn(&mut self, f: &FnDef, is_interrupt: bool) {
        let ret_type = f.ret_type.as_ref().map(Ty::from_ast).unwrap_or(Ty::Void);
        let mut scope = FnScope {
            vars: HashMap::new(),
            ret_type,
            is_interrupt,
        };

        for param in &f.params {
            let ty = Ty::from_ast(&param.ty);
            // detect board parameter: &mut BoardName
            if let Ty::MutRef(inner) = &ty {
                if let Ty::Named(name) = inner.as_ref() {
                    if self.board_peripherals.contains_key(name) {
                        scope.vars.insert(param.name.clone(), Ty::Board(name.clone()));
                        continue;
                    }
                }
            }
            scope.vars.insert(param.name.clone(), ty);
        }

        self.check_block(&f.body, &mut scope);
    }

    fn check_block(&mut self, block: &Block, scope: &mut FnScope) {
        for stmt in &block.stmts {
            self.check_stmt(stmt, scope);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt, scope: &mut FnScope) {
        match stmt {
            Stmt::Let { name, ty, value, span, .. } => {
                let val_ty = self.check_expr(value, scope);

                if let Some(declared) = ty {
                    let declared_ty = Ty::from_ast(declared);
                    if val_ty != Ty::Unknown && declared_ty != Ty::Unknown && val_ty != declared_ty {
                        self.err(*span, format!(
                            "type mismatch: declared {} as {:?} but value is {:?}",
                            name, declared_ty, val_ty
                        ));
                    }
                }

                // check peripheral ownership — if the value is a peripheral claim
                self.check_peripheral_claim(value, scope, *span);

                scope.vars.insert(name.clone(), val_ty);
            }

            Stmt::Assign { target, value, span, .. } => {
                let target_ty = self.check_expr(target, scope);
                let val_ty = self.check_expr(value, scope);
                if target_ty != Ty::Unknown && val_ty != Ty::Unknown && target_ty != val_ty {
                    self.err(*span, format!(
                        "cannot assign {:?} to {:?}", val_ty, target_ty
                    ));
                }
            }

            Stmt::Return(Some(expr), span) => {
                let ty = self.check_expr(expr, scope);
                if ty != Ty::Unknown && scope.ret_type != Ty::Unknown && ty != scope.ret_type {
                    self.err(*span, format!(
                        "return type mismatch: expected {:?}, got {:?}", scope.ret_type, ty
                    ));
                }
            }

            Stmt::If { condition, then_block, else_block, span } => {
                let cond_ty = self.check_expr(condition, scope);
                if cond_ty != Ty::Bool && cond_ty != Ty::Unknown {
                    self.err(*span, format!("if condition must be bool, got {:?}", cond_ty));
                }
                self.check_block(then_block, scope);
                if let Some(eb) = else_block {
                    match eb {
                        ElseBranch::Else(block) => self.check_block(block, scope),
                        ElseBranch::ElseIf(stmt) => self.check_stmt(stmt, scope),
                    }
                }
            }

            Stmt::While { condition, body, span } => {
                let cond_ty = self.check_expr(condition, scope);
                if cond_ty != Ty::Bool && cond_ty != Ty::Unknown {
                    self.err(*span, format!("while condition must be bool, got {:?}", cond_ty));
                }
                self.check_block(body, scope);
            }

            Stmt::Loop(body, _) => self.check_block(body, scope),

            Stmt::For { var, start, end, body, .. } => {
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

            Stmt::Expr(expr) => { self.check_expr(expr, scope); }
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
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
                    | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                    | BinOp::WrapAdd | BinOp::WrapSub | BinOp::WrapMul => {
                        if lt != Ty::Unknown && !lt.is_numeric() {
                            self.err(*span, format!("arithmetic on non-numeric type {:?}", lt));
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
                for arg in args { self.check_expr(arg, scope); }
                Ty::Unknown // method return types need full resolution
            }

            Expr::Call(callee, args, _) => {
                // don't error on unknown function names — resolved at link time
                if !matches!(callee.as_ref(), Expr::Ident(_, _)) {
                    self.check_expr(callee, scope);
                }
                for arg in args { self.check_expr(arg, scope); }
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
        TypeChecker::new().check(&program)
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
    fn valid_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        if let Err(errors) = check(&source) {
            for e in &errors { eprintln!("  {}", e); }
            panic!("{} type errors", errors.len());
        }
    }
}
