use std::collections::HashMap;
use crate::parser::ast::{self, BinOp, UnaryOp, Stmt, Expr, Program, TopItem};
use super::{Function, Block, Value, Op, Terminator};
use super::types::IrType;

pub struct Lowering {
    pub functions: Vec<Function>,
}

struct FnBuilder<'a> {
    func: &'a mut Function,
    current_block: Block,
    // variable name → current SSA value
    vars: HashMap<String, Value>,
}

impl Lowering {
    pub fn lower(program: &Program) -> Self {
        let mut functions = Vec::new();

        for item in &program.items {
            match item {
                TopItem::Function(f) => {
                    functions.push(lower_fn(f));
                }
                TopItem::Interrupt(i) => {
                    // lower interrupt as a regular function (wrapper generated in codegen)
                    let fake_fn = ast::FnDef {
                        name: i.fn_name.clone(),
                        attrs: Vec::new(),
                        params: Vec::new(),
                        ret_type: None,
                        is_error_return: false,
                        body: i.body.clone(),
                        span: i.span,
                    };
                    functions.push(lower_fn(&fake_fn));
                }
                _ => {} // board, struct, etc handled in type checking
            }
        }

        Self { functions }
    }
}

fn ast_type_to_ir(ty: &Option<ast::Type>) -> IrType {
    match ty {
        None => IrType::Void,
        Some(t) => match t {
            ast::Type::Primitive(p) => match p {
                ast::PrimitiveType::U8 | ast::PrimitiveType::I8 => IrType::I8,
                ast::PrimitiveType::U16 | ast::PrimitiveType::I16 => IrType::I16,
                ast::PrimitiveType::U32 | ast::PrimitiveType::I32 |
                ast::PrimitiveType::Usize | ast::PrimitiveType::Isize => IrType::I32,
                ast::PrimitiveType::U64 | ast::PrimitiveType::I64 => IrType::I64,
                ast::PrimitiveType::Bool => IrType::Bool,
                ast::PrimitiveType::Void => IrType::Void,
            },
            ast::Type::Ref(_, _) | ast::Type::Ptr(_, _) => IrType::Ptr,
            // named types, arrays etc → i32 for now (full type resolution later)
            _ => IrType::I32,
        },
    }
}

fn lower_fn(f: &ast::FnDef) -> Function {
    let params: Vec<(String, IrType)> = f.params.iter()
        .map(|p| (p.name.clone(), ast_type_to_ir(&Some(p.ty.clone()))))
        .collect();
    let ret_type = ast_type_to_ir(&f.ret_type);

    let mut func = Function::new(f.name.clone(), params.clone(), ret_type);
    let entry = func.new_block();

    // bind function parameters as values
    let mut vars = HashMap::new();
    for (name, ty) in &params {
        let val = func.new_value(*ty, Some(name.clone()));
        vars.insert(name.clone(), val);
    }

    let mut builder = FnBuilder {
        func: &mut func,
        current_block: entry,
        vars,
    };

    for stmt in &f.body.stmts {
        builder.lower_stmt(stmt);
    }

    // if no terminator set, add implicit return void
    if matches!(builder.func.blocks[builder.current_block.0 as usize].terminator, Terminator::None) {
        builder.func.set_terminator(builder.current_block, Terminator::Return(None));
    }

    func
}

impl<'a> FnBuilder<'a> {
    fn emit(&mut self, op: Op, ty: IrType) -> Value {
        self.func.push_inst(self.current_block, op, ty)
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, ty, .. } => {
                let val = self.lower_expr(value);
                self.vars.insert(name.clone(), val);
            }

            Stmt::Assign { target, value, .. } => {
                let _addr = self.lower_expr(target);
                let _val = self.lower_expr(value);
                // TODO: proper store via address resolution
            }

            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }

            Stmt::Return(Some(expr), _) => {
                let val = self.lower_expr(expr);
                self.func.set_terminator(self.current_block, Terminator::Return(Some(val)));
                // new unreachable block for any following stmts
                self.current_block = self.func.new_block();
            }

            Stmt::Return(None, _) => {
                self.func.set_terminator(self.current_block, Terminator::Return(None));
                self.current_block = self.func.new_block();
            }

            Stmt::If { condition, then_block, else_block, .. } => {
                let cond = self.lower_expr(condition);
                let then_bb = self.func.new_block();
                let else_bb = self.func.new_block();
                let merge_bb = self.func.new_block();

                self.func.set_terminator(self.current_block, Terminator::BranchIf {
                    cond,
                    then_block: then_bb,
                    then_args: vec![],
                    else_block: else_bb,
                    else_args: vec![],
                });

                // then
                self.current_block = then_bb;
                for s in &then_block.stmts {
                    self.lower_stmt(s);
                }
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
                }

                // else
                self.current_block = else_bb;
                if let Some(eb) = else_block {
                    match eb {
                        ast::ElseBranch::Else(block) => {
                            for s in &block.stmts {
                                self.lower_stmt(s);
                            }
                        }
                        ast::ElseBranch::ElseIf(stmt) => {
                            self.lower_stmt(stmt);
                        }
                    }
                }
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
                }

                self.current_block = merge_bb;
            }

            Stmt::Loop(body, _) => {
                let loop_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                self.func.set_terminator(self.current_block, Terminator::Jump(loop_bb, vec![]));
                self.current_block = loop_bb;

                for s in &body.stmts {
                    self.lower_stmt(s);
                }

                // loop back if no explicit break/return
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(loop_bb, vec![]));
                }

                self.current_block = exit_bb;
            }

            Stmt::While { condition, body, .. } => {
                let cond_bb = self.func.new_block();
                let body_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                self.func.set_terminator(self.current_block, Terminator::Jump(cond_bb, vec![]));

                self.current_block = cond_bb;
                let cond = self.lower_expr(condition);
                self.func.set_terminator(self.current_block, Terminator::BranchIf {
                    cond,
                    then_block: body_bb,
                    then_args: vec![],
                    else_block: exit_bb,
                    else_args: vec![],
                });

                self.current_block = body_bb;
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(cond_bb, vec![]));
                }

                self.current_block = exit_bb;
            }

            Stmt::For { var, start, end, body, .. } => {
                // desugar: loop header checks i < end, body increments
                let init = self.lower_expr(start);
                let limit = self.lower_expr(end);

                let header_bb = self.func.new_block();
                let body_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                // entry → header with initial value
                self.func.set_terminator(self.current_block, Terminator::Jump(header_bb, vec![init]));

                // header: block param = loop variable
                let loop_var = self.func.add_block_param(header_bb, IrType::I32);
                self.vars.insert(var.clone(), loop_var);

                self.current_block = header_bb;
                let cond = self.emit(Op::Lt(loop_var, limit), IrType::Bool);
                self.func.set_terminator(self.current_block, Terminator::BranchIf {
                    cond,
                    then_block: body_bb,
                    then_args: vec![],
                    else_block: exit_bb,
                    else_args: vec![],
                });

                // body
                self.current_block = body_bb;
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                // increment and loop back
                let one = self.emit(Op::ConstI32(1), IrType::I32);
                let next = self.emit(Op::Add(loop_var, one), IrType::I32);
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(header_bb, vec![next]));
                }

                self.current_block = exit_bb;
            }

            _ => {} // match, defer, critical_section — TODO
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::IntLit(v, _) => {
                self.emit(Op::ConstI32(*v as i32), IrType::I32)
            }

            Expr::BoolLit(v, _) => {
                self.emit(Op::ConstBool(*v), IrType::Bool)
            }

            Expr::Ident(name, _) => {
                *self.vars.get(name).unwrap_or(&Value(0))
            }

            Expr::Binary(lhs, op, rhs, _) => {
                let l = self.lower_expr(lhs);
                let r = self.lower_expr(rhs);
                let ir_op = match op {
                    BinOp::Add | BinOp::WrapAdd => Op::Add(l, r),
                    BinOp::Sub | BinOp::WrapSub => Op::Sub(l, r),
                    BinOp::Mul | BinOp::WrapMul => Op::Mul(l, r),
                    BinOp::Div => Op::Div(l, r),
                    BinOp::Rem => Op::Rem(l, r),
                    BinOp::BitAnd => Op::And(l, r),
                    BinOp::BitOr => Op::Or(l, r),
                    BinOp::BitXor => Op::Xor(l, r),
                    BinOp::Shl => Op::Shl(l, r),
                    BinOp::Shr => Op::Shr(l, r),
                    BinOp::Eq => Op::Eq(l, r),
                    BinOp::Ne => Op::Ne(l, r),
                    BinOp::Lt => Op::Lt(l, r),
                    BinOp::Gt => Op::Lt(r, l), // flip
                    BinOp::Le => Op::Ge(r, l), // flip
                    BinOp::Ge => Op::Ge(l, r),
                    BinOp::And => Op::And(l, r), // TODO: short-circuit
                    BinOp::Or => Op::Or(l, r),
                    _ => Op::Nop,
                };
                let ty = match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt
                    | BinOp::Le | BinOp::Ge => IrType::Bool,
                    _ => IrType::I32,
                };
                self.emit(ir_op, ty)
            }

            Expr::Unary(op, inner, _) => {
                let v = self.lower_expr(inner);
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg(v), IrType::I32),
                    UnaryOp::Not => self.emit(Op::Not(v), IrType::Bool),
                    UnaryOp::BitNot => self.emit(Op::Not(v), IrType::I32),
                    _ => v, // &, &mut, * — handled in type checker
                }
            }

            // method calls and function calls → Call op
            Expr::Call(callee, args, _) => {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
                if let Expr::Ident(name, _) = callee.as_ref() {
                    self.emit(Op::Call(name.clone(), arg_vals), IrType::I32)
                } else {
                    // indirect call — lower callee as value
                    let _callee_val = self.lower_expr(callee);
                    self.emit(Op::Nop, IrType::Void) // TODO: indirect calls
                }
            }

            Expr::MethodCall(obj, method, args, _) => {
                let _obj_val = self.lower_expr(obj);
                let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
                // desugar: obj.method(args) → method(obj, args)
                // for now just emit as a named call
                self.emit(Op::Call(method.clone(), arg_vals), IrType::Void)
            }

            Expr::Field(obj, _field, _) => {
                // TODO: struct field offset calculation
                self.lower_expr(obj)
            }

            Expr::StringLit(_, _) => {
                // strings become global data + pointer
                self.emit(Op::ConstI32(0), IrType::Ptr) // placeholder
            }

            _ => {
                // DotEnum, ArrayLit, StructLit, etc — TODO
                self.emit(Op::Nop, IrType::Void)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn lower(src: &str) -> Lowering {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        Lowering::lower(&program)
    }

    #[test]
    fn lower_simple_fn() {
        let ir = lower("fn add(a: u32, b: u32) u32 { let x = a + b; return x; }");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
        // should have entry block with add inst + return
        assert!(!ir.functions[0].blocks.is_empty());
        println!("{}", ir.functions[0]);
    }

    #[test]
    fn lower_if_else() {
        let ir = lower("fn f(x: u32) { if x == 0 { } else { } }");
        assert_eq!(ir.functions.len(), 1);
        // entry + then + else + merge = 4 blocks
        assert!(ir.functions[0].blocks.len() >= 4);
    }

    #[test]
    fn lower_loop() {
        let ir = lower("fn f() { loop { } }");
        assert_eq!(ir.functions.len(), 1);
        // entry + loop body + exit = 3 blocks
        assert!(ir.functions[0].blocks.len() >= 2);
    }

    #[test]
    fn lower_for_range() {
        let ir = lower("fn f() { for i in 0..10 { } }");
        assert_eq!(ir.functions.len(), 1);
        // has a block parameter for the loop variable
        let has_block_param = ir.functions[0].blocks.iter().any(|b| !b.params.is_empty());
        assert!(has_block_param, "for-loop should create block parameter for loop var");
        println!("{}", ir.functions[0]);
    }

    #[test]
    fn lower_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        let tokens = Lexer::tokenize(&source).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);
        // main + on_tick = 2 functions
        assert_eq!(ir.functions.len(), 2);
        for f in &ir.functions {
            println!("{}\n", f);
        }
    }
}
