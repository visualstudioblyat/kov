use std::collections::HashMap;
use crate::parser::ast::{self, BinOp, UnaryOp, Stmt, Expr, Program, TopItem};
use crate::codegen::mmio::{PeripheralMap, MmioValue, resolve_method};
use super::{Function, Block, Value, Op, Terminator};
use super::types::IrType;

pub struct Lowering {
    pub functions: Vec<Function>,
    pub globals: super::globals::GlobalTable,
}

// tracks what a variable represents for MMIO resolution
#[derive(Clone)]
enum VarKind {
    Value(Value),
    Board(String),                      // board variable name
    PeripheralHandle(String, u32, u32), // (peripheral_name, base_addr, pin_num)
}

struct FnBuilder<'a> {
    func: &'a mut Function,
    current_block: Block,
    vars: HashMap<String, Value>,
    var_kinds: HashMap<String, VarKind>,
    periph_map: &'a PeripheralMap,
    globals: &'a super::globals::GlobalTable,
    loop_stack: Vec<(Block, Block)>, // (header/continue_target, exit_block)
}

impl Lowering {
    pub fn lower(program: &Program) -> Self {
        use super::globals::{GlobalTable, GlobalInit};

        let periph_map = PeripheralMap::from_program(program);
        let mut functions = Vec::new();
        let mut globals = GlobalTable::new();

        // first pass: collect globals
        for item in &program.items {
            if let TopItem::Static(s) = item {
                let ty = ast_type_to_ir(&Some(s.ty.clone()));
                let init = match &s.value {
                    Expr::IntLit(v, _) => GlobalInit::Int(*v as i32),
                    _ => GlobalInit::Zero,
                };
                globals.add_global(s.name.clone(), ty, init, s.mutable);
            }
        }

        // second pass: lower functions (including statics inside interrupt bodies)
        for item in &program.items {
            match item {
                TopItem::Function(f) => {
                    collect_body_statics(&f.body, &mut globals);
                    functions.push(lower_fn(f, &periph_map, &globals));
                }
                TopItem::Interrupt(i) => {
                    collect_body_statics(&i.body, &mut globals);
                    let fake_fn = ast::FnDef {
                        name: i.fn_name.clone(),
                        attrs: Vec::new(),
                        params: Vec::new(),
                        ret_type: None,
                        is_error_return: false,
                        body: i.body.clone(),
                        span: i.span,
                    };
                    functions.push(lower_fn(&fake_fn, &periph_map, &globals));
                }
                _ => {}
            }
        }

        Self { functions, globals }
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

// scan a block for static declarations and add them to the global table
fn collect_body_statics(block: &ast::Block, globals: &mut super::globals::GlobalTable) {
    use super::globals::GlobalInit;
    for stmt in &block.stmts {
        if let Stmt::Let { name, ty: Some(ty), value, mutable, .. } = stmt {
            // detect "static mut counter: u32 = 0" pattern
            // (the parser lowered static inside functions to Let with ty)
            let ir_ty = ast_type_to_ir(&Some(ty.clone()));
            if *mutable {
                let init = match value {
                    Expr::IntLit(v, _) => GlobalInit::Int(*v as i32),
                    _ => GlobalInit::Zero,
                };
                // only add if it looks like a static (has explicit type + mutable)
                if globals.find(name).is_none() {
                    globals.add_global(name.clone(), ir_ty, init, true);
                }
            }
        }
    }
}

fn lower_fn(f: &ast::FnDef, periph_map: &PeripheralMap, globals: &super::globals::GlobalTable) -> Function {
    let params: Vec<(String, IrType)> = f.params.iter()
        .map(|p| (p.name.clone(), ast_type_to_ir(&Some(p.ty.clone()))))
        .collect();
    let ret_type = ast_type_to_ir(&f.ret_type);

    let mut func = Function::new(f.name.clone(), params.clone(), ret_type);
    let entry = func.new_block();

    let mut vars = HashMap::new();
    let mut var_kinds = HashMap::new();

    for (name, ty) in &params {
        let val = func.new_value(*ty, Some(name.clone()));
        vars.insert(name.clone(), val);
        // detect board parameter
        if let Some(param) = f.params.iter().find(|p| &p.name == name) {
            if let ast::Type::Ref(inner, true) = &param.ty {
                if let ast::Type::Named(board_name, _) = inner.as_ref() {
                    if periph_map.board_name.as_deref() == Some(board_name.as_str()) {
                        var_kinds.insert(name.clone(), VarKind::Board(board_name.clone()));
                    }
                }
            }
        }
    }

    let mut builder = FnBuilder {
        func: &mut func,
        current_block: entry,
        vars,
        var_kinds,
        periph_map,
        globals,
        loop_stack: Vec::new(),
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

    // detect: b.gpio.pin(N, .output) → PeripheralHandle("gpio", base_addr, N)
    fn detect_peripheral_handle(&self, expr: &Expr) -> Option<VarKind> {
        if let Expr::MethodCall(obj, method, args, _) = expr {
            if method == "pin" || method == "open" {
                if let Expr::Field(board_expr, periph_name, _) = obj.as_ref() {
                    if let Expr::Ident(var_name, _) = board_expr.as_ref() {
                        if let Some(VarKind::Board(_)) = self.var_kinds.get(var_name) {
                            if let Some(base) = self.periph_map.get_address(periph_name) {
                                let pin = args.first().and_then(|a| {
                                    if let Expr::IntLit(n, _) = a { Some(*n as u32) } else { None }
                                }).unwrap_or(0);
                                return Some(VarKind::PeripheralHandle(
                                    periph_name.clone(), base, pin
                                ));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    // try to lower a method call as an MMIO operation
    fn try_lower_mmio(&mut self, obj: &Expr, method: &str, args: &[Expr]) -> Option<Value> {
        let var_name = match obj {
            Expr::Ident(name, _) => name,
            _ => return None,
        };

        let kind = self.var_kinds.get(var_name)?.clone();
        if let VarKind::PeripheralHandle(periph, base, pin) = kind {
            if let Some(ops) = resolve_method(&periph, method, base, Some(pin)) {
                for op in &ops {
                    // load address into a register
                    let addr = self.emit(Op::ConstI32(op.address as i32), IrType::Ptr);
                    match &op.value {
                        MmioValue::Constant(v) => {
                            let val = self.emit(Op::ConstI32(*v as i32), IrType::I32);
                            self.emit(Op::VolatileStore(addr, val), IrType::Void);
                        }
                        MmioValue::Register(_) => {
                            // value comes from a function argument
                            if let Some(arg) = args.first() {
                                let val = self.lower_expr(arg);
                                self.emit(Op::VolatileStore(addr, val), IrType::Void);
                            }
                        }
                    }
                }
                return Some(self.emit(Op::Nop, IrType::Void));
            }
        }
        None
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, ty: _, .. } => {
                // detect peripheral handle creation: b.gpio.pin(N, ...)
                if let Some(kind) = self.detect_peripheral_handle(value) {
                    self.var_kinds.insert(name.clone(), kind);
                }
                let val = self.lower_expr(value);
                self.vars.insert(name.clone(), val);
            }

            Stmt::Assign { target, value, .. } => {
                if let Expr::Ident(name, _) = target {
                    if self.globals.find(name).is_some() && !self.vars.contains_key(name) {
                        // global variable assignment: emit GlobalAddr + Store
                        let val = self.lower_expr(value);
                        let addr = self.emit(Op::GlobalAddr(name.clone()), IrType::Ptr);
                        self.emit(Op::Store(addr, val), IrType::Void);
                        return;
                    }
                }
                let _addr = self.lower_expr(target);
                let _val = self.lower_expr(value);
                // TODO: proper store via address resolution for non-globals
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

                self.loop_stack.push((loop_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();

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
                self.loop_stack.push((cond_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();
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
                self.loop_stack.push((header_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();
                // increment and loop back
                let one = self.emit(Op::ConstI32(1), IrType::I32);
                let next = self.emit(Op::Add(loop_var, one), IrType::I32);
                if matches!(self.func.blocks[self.current_block.0 as usize].terminator, Terminator::None) {
                    self.func.set_terminator(self.current_block, Terminator::Jump(header_bb, vec![next]));
                }

                self.current_block = exit_bb;
            }

            Stmt::Break(_) => {
                if let Some(&(_, exit_bb)) = self.loop_stack.last() {
                    self.func.set_terminator(self.current_block, Terminator::Jump(exit_bb, vec![]));
                    self.current_block = self.func.new_block();
                }
            }

            Stmt::Continue(_) => {
                if let Some(&(header_bb, _)) = self.loop_stack.last() {
                    self.func.set_terminator(self.current_block, Terminator::Jump(header_bb, vec![]));
                    self.current_block = self.func.new_block();
                }
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
                if let Some(&val) = self.vars.get(name) {
                    val
                } else if let Some(g) = self.globals.find(name) {
                    // global variable: emit GlobalAddr + Load
                    let ty = g.ty;
                    let addr = self.emit(Op::GlobalAddr(name.clone()), IrType::Ptr);
                    self.emit(Op::Load(addr, ty), ty)
                } else {
                    Value(0)
                }
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
                // check if this is a peripheral method → emit MMIO
                if let Some(mmio_result) = self.try_lower_mmio(obj, method, args) {
                    return mmio_result;
                }
                let _obj_val = self.lower_expr(obj);
                let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
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

    #[test]
    fn lower_global_read() {
        let ir = lower("static mut counter: u32 = 0;\nfn get() u32 { return counter; }");
        assert_eq!(ir.functions.len(), 1);
        // should have a GlobalAddr op followed by a Load
        let func = &ir.functions[0];
        let has_global_addr = func.blocks.iter().any(|b| {
            b.insts.iter().any(|i| matches!(&i.op, Op::GlobalAddr(n) if n == "counter"))
        });
        assert!(has_global_addr, "should emit GlobalAddr for global variable read");
        let has_load = func.blocks.iter().any(|b| {
            b.insts.iter().any(|i| matches!(&i.op, Op::Load(_, _)))
        });
        assert!(has_load, "should emit Load after GlobalAddr");
        println!("{}", func);
    }

    #[test]
    fn lower_global_write() {
        let ir = lower("static mut counter: u32 = 0;\nfn set() { counter = 42; }");
        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let has_store = func.blocks.iter().any(|b| {
            b.insts.iter().any(|i| matches!(&i.op, Op::Store(_, _)))
        });
        assert!(has_store, "should emit Store for global variable write");
        println!("{}", func);
    }

    #[test]
    fn lower_break_in_loop() {
        let ir = lower("fn f() { loop { break; } }");
        let func = &ir.functions[0];
        // break should create a jump to exit block, not loop infinitely
        // we should have at least 3 blocks: entry, loop body, exit
        assert!(func.blocks.len() >= 3, "break should create exit block");
        println!("{}", func);
    }

    #[test]
    fn lower_continue_in_while() {
        let ir = lower("fn f(x: u32) { while x > 0 { continue; } }");
        let func = &ir.functions[0];
        // continue should jump back to condition block
        assert!(func.blocks.len() >= 3, "while with continue should have cond/body/exit blocks");
        println!("{}", func);
    }

    #[test]
    fn lower_global_increment_with_break() {
        let ir = lower(
            "static mut ticks: u32 = 0;\nfn f() { loop { ticks = ticks + 1; if ticks == 10 { break; } } }"
        );
        let func = &ir.functions[0];
        // should have GlobalAddr, Load, Store ops and a break jump
        let has_global = func.blocks.iter().any(|b| {
            b.insts.iter().any(|i| matches!(&i.op, Op::GlobalAddr(n) if n == "ticks"))
        });
        assert!(has_global, "should reference 'ticks' global");
        println!("{}", func);
    }
}
