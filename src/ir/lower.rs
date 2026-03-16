use super::types::IrType;
use super::{Block, Function, Op, Terminator, Value};
use crate::codegen::mmio::{MmioValue, PeripheralMap, resolve_method};
use crate::parser::ast::{self, BinOp, Expr, Program, Stmt, TopItem, UnaryOp};
use std::collections::HashMap;

pub struct Lowering {
    pub functions: Vec<Function>,
    pub globals: super::globals::GlobalTable,
    pub struct_layouts: HashMap<String, StructLayout>,
}

#[derive(Debug, Clone)]
pub struct StructLayout {
    pub size: u32,
    pub fields: Vec<(String, u32, IrType)>, // (name, offset, type)
}

impl StructLayout {
    fn from_def(def: &ast::StructDef) -> Self {
        let mut offset = 0u32;
        let mut fields = Vec::new();
        for f in &def.fields {
            let ty = ast_type_to_ir(&Some(f.ty.clone()));
            let size = ty.size_bytes().max(1);
            // align to natural boundary
            let align = size;
            offset = (offset + align - 1) & !(align - 1);
            fields.push((f.name.clone(), offset, ty));
            offset += size;
        }
        // align total size to 4
        let size = (offset + 3) & !3;
        Self { size, fields }
    }

    pub fn field_offset(&self, name: &str) -> Option<(u32, IrType)> {
        self.fields
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, off, ty)| (*off, *ty))
    }
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
    var_types: HashMap<String, String>, // var_name → struct type name (for field access)
    periph_map: &'a PeripheralMap,
    globals: &'a super::globals::GlobalTable,
    struct_layouts: &'a HashMap<String, StructLayout>,
    loop_stack: Vec<(Option<String>, Block, Block)>, // (label, header/continue_target, exit_block)
}

impl Lowering {
    pub fn lower(program: &Program) -> Self {
        use super::globals::{GlobalInit, GlobalTable};

        let periph_map = PeripheralMap::from_program(program);
        let mut functions = Vec::new();
        let mut globals = GlobalTable::new();
        let mut struct_layouts = HashMap::new();

        // first pass: collect structs and globals
        for item in &program.items {
            if let TopItem::Struct(s) = item {
                struct_layouts.insert(s.name.clone(), StructLayout::from_def(s));
            }
        }
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

        // collect string literals from all function bodies
        for item in &program.items {
            let body = match item {
                TopItem::Function(f) => Some(&f.body),
                TopItem::Interrupt(i) => Some(&i.body),
                _ => None,
            };
            if let Some(body) = body {
                collect_strings(&body.stmts, &mut globals);
            }
        }

        // second pass: lower functions (including statics inside interrupt bodies)
        for item in &program.items {
            match item {
                TopItem::Function(f) => {
                    collect_body_statics(&f.body, &mut globals);
                    functions.push(lower_fn(f, &periph_map, &globals, &struct_layouts));
                }
                TopItem::Interrupt(i) => {
                    collect_body_statics(&i.body, &mut globals);
                    let fake_fn = ast::FnDef {
                        name: i.fn_name.clone(),
                        type_params: Vec::new(),
                        attrs: Vec::new(),
                        params: Vec::new(),
                        ret_type: None,
                        is_error_return: false,
                        body: i.body.clone(),
                        span: i.span,
                    };
                    functions.push(lower_fn(&fake_fn, &periph_map, &globals, &struct_layouts));
                }
                _ => {}
            }
        }

        Self {
            functions,
            globals,
            struct_layouts,
        }
    }
}

fn ast_type_to_ir(ty: &Option<ast::Type>) -> IrType {
    match ty {
        None => IrType::Void,
        Some(t) => match t {
            ast::Type::Primitive(p) => match p {
                ast::PrimitiveType::U8 | ast::PrimitiveType::I8 => IrType::I8,
                ast::PrimitiveType::U16 | ast::PrimitiveType::I16 => IrType::I16,
                ast::PrimitiveType::U32
                | ast::PrimitiveType::I32
                | ast::PrimitiveType::Usize
                | ast::PrimitiveType::Isize => IrType::I32,
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

// recursively collect string literals from statements and add them to globals
fn collect_strings(stmts: &[Stmt], globals: &mut super::globals::GlobalTable) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(expr) | Stmt::Return(Some(expr), _) => collect_strings_expr(expr, globals),
            Stmt::Let { value, .. } => collect_strings_expr(value, globals),
            Stmt::Assign { value, .. } => collect_strings_expr(value, globals),
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                collect_strings_expr(condition, globals);
                collect_strings(&then_block.stmts, globals);
                if let Some(eb) = else_block {
                    match eb {
                        ast::ElseBranch::Else(b) => collect_strings(&b.stmts, globals),
                        ast::ElseBranch::ElseIf(s) => collect_strings(&[*s.clone()], globals),
                    }
                }
            }
            Stmt::Loop(_, body, _) => collect_strings(&body.stmts, globals),
            Stmt::While { body, .. } => collect_strings(&body.stmts, globals),
            Stmt::For { body, .. } => collect_strings(&body.stmts, globals),
            _ => {}
        }
    }
}

fn collect_strings_expr(expr: &Expr, globals: &mut super::globals::GlobalTable) {
    match expr {
        Expr::StringLit(s, _) => {
            globals.add_string(s.as_bytes());
        }
        Expr::Call(_, args, _) => {
            for a in args {
                collect_strings_expr(a, globals);
            }
        }
        Expr::MethodCall(obj, _, args, _) => {
            collect_strings_expr(obj, globals);
            for a in args {
                collect_strings_expr(a, globals);
            }
        }
        Expr::Binary(l, _, r, _) => {
            collect_strings_expr(l, globals);
            collect_strings_expr(r, globals);
        }
        _ => {}
    }
}

// scan a block for static declarations and add them to the global table
fn collect_body_statics(block: &ast::Block, globals: &mut super::globals::GlobalTable) {
    use super::globals::GlobalInit;
    for stmt in &block.stmts {
        if let Stmt::Let {
            name,
            ty: Some(ty),
            value,
            mutable,
            ..
        } = stmt
        {
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

fn lower_fn(
    f: &ast::FnDef,
    periph_map: &PeripheralMap,
    globals: &super::globals::GlobalTable,
    struct_layouts: &HashMap<String, StructLayout>,
) -> Function {
    let params: Vec<(String, IrType)> = f
        .params
        .iter()
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
        var_types: HashMap::new(),
        periph_map,
        globals,
        struct_layouts,
        loop_stack: Vec::new(),
    };

    for stmt in &f.body.stmts {
        builder.lower_stmt(stmt);
    }

    // if no terminator set, add implicit return void
    if matches!(
        builder.func.blocks[builder.current_block.0 as usize].terminator,
        Terminator::None
    ) {
        builder
            .func
            .set_terminator(builder.current_block, Terminator::Return(None));
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
                                let pin = args
                                    .first()
                                    .and_then(|a| {
                                        if let Expr::IntLit(n, _) = a {
                                            Some(*n as u32)
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                return Some(VarKind::PeripheralHandle(
                                    periph_name.clone(),
                                    base,
                                    pin,
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
            Stmt::Let {
                name, value, ty: _, ..
            } => {
                // detect peripheral handle creation: b.gpio.pin(N, ...)
                if let Some(kind) = self.detect_peripheral_handle(value) {
                    self.var_kinds.insert(name.clone(), kind);
                }
                // track struct type for field access
                if let Expr::StructLit(struct_name, _, _) = value {
                    self.var_types.insert(name.clone(), struct_name.clone());
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
                self.func
                    .set_terminator(self.current_block, Terminator::Return(Some(val)));
                // new unreachable block for any following stmts
                self.current_block = self.func.new_block();
            }

            Stmt::Return(None, _) => {
                self.func
                    .set_terminator(self.current_block, Terminator::Return(None));
                self.current_block = self.func.new_block();
            }

            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let cond = self.lower_expr(condition);
                let then_bb = self.func.new_block();
                let else_bb = self.func.new_block();
                let merge_bb = self.func.new_block();

                self.func.set_terminator(
                    self.current_block,
                    Terminator::BranchIf {
                        cond,
                        then_block: then_bb,
                        then_args: vec![],
                        else_block: else_bb,
                        else_args: vec![],
                    },
                );

                // then
                self.current_block = then_bb;
                for s in &then_block.stmts {
                    self.lower_stmt(s);
                }
                if matches!(
                    self.func.blocks[self.current_block.0 as usize].terminator,
                    Terminator::None
                ) {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
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
                if matches!(
                    self.func.blocks[self.current_block.0 as usize].terminator,
                    Terminator::None
                ) {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
                }

                self.current_block = merge_bb;
            }

            Stmt::Loop(label, body, _) => {
                let loop_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                self.func
                    .set_terminator(self.current_block, Terminator::Jump(loop_bb, vec![]));
                self.current_block = loop_bb;

                self.loop_stack.push((label.clone(), loop_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();

                // loop back if no explicit break/return
                if matches!(
                    self.func.blocks[self.current_block.0 as usize].terminator,
                    Terminator::None
                ) {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(loop_bb, vec![]));
                }

                self.current_block = exit_bb;
            }

            Stmt::While {
                label,
                condition,
                body,
                ..
            } => {
                let cond_bb = self.func.new_block();
                let body_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                self.func
                    .set_terminator(self.current_block, Terminator::Jump(cond_bb, vec![]));

                self.current_block = cond_bb;
                let cond = self.lower_expr(condition);
                self.func.set_terminator(
                    self.current_block,
                    Terminator::BranchIf {
                        cond,
                        then_block: body_bb,
                        then_args: vec![],
                        else_block: exit_bb,
                        else_args: vec![],
                    },
                );

                self.current_block = body_bb;
                self.loop_stack.push((label.clone(), cond_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();
                if matches!(
                    self.func.blocks[self.current_block.0 as usize].terminator,
                    Terminator::None
                ) {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(cond_bb, vec![]));
                }

                self.current_block = exit_bb;
            }

            Stmt::For {
                label,
                var,
                start,
                end,
                body,
                ..
            } => {
                // desugar: loop header checks i < end, body increments
                let init = self.lower_expr(start);
                let limit = self.lower_expr(end);

                let header_bb = self.func.new_block();
                let body_bb = self.func.new_block();
                let exit_bb = self.func.new_block();

                // entry → header with initial value
                self.func
                    .set_terminator(self.current_block, Terminator::Jump(header_bb, vec![init]));

                // header: block param = loop variable
                let loop_var = self.func.add_block_param(header_bb, IrType::I32);
                self.vars.insert(var.clone(), loop_var);

                self.current_block = header_bb;
                let cond = self.emit(Op::Lt(loop_var, limit), IrType::Bool);
                self.func.set_terminator(
                    self.current_block,
                    Terminator::BranchIf {
                        cond,
                        then_block: body_bb,
                        then_args: vec![],
                        else_block: exit_bb,
                        else_args: vec![],
                    },
                );

                // body
                self.current_block = body_bb;
                self.loop_stack.push((label.clone(), header_bb, exit_bb));
                for s in &body.stmts {
                    self.lower_stmt(s);
                }
                self.loop_stack.pop();
                // increment and loop back
                let one = self.emit(Op::ConstI32(1), IrType::I32);
                let next = self.emit(Op::Add(loop_var, one), IrType::I32);
                if matches!(
                    self.func.blocks[self.current_block.0 as usize].terminator,
                    Terminator::None
                ) {
                    self.func.set_terminator(
                        self.current_block,
                        Terminator::Jump(header_bb, vec![next]),
                    );
                }

                self.current_block = exit_bb;
            }

            Stmt::Break(label, _) => {
                let target = if let Some(lbl) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|(l, _, _)| l.as_deref() == Some(lbl.as_str()))
                        .map(|&(_, _, exit_bb)| exit_bb)
                } else {
                    self.loop_stack.last().map(|&(_, _, exit_bb)| exit_bb)
                };
                if let Some(exit_bb) = target {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(exit_bb, vec![]));
                    self.current_block = self.func.new_block();
                }
            }

            Stmt::Continue(label, _) => {
                let target = if let Some(lbl) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|(l, _, _)| l.as_deref() == Some(lbl.as_str()))
                        .map(|&(_, header_bb, _)| header_bb)
                } else {
                    self.loop_stack.last().map(|&(_, header_bb, _)| header_bb)
                };
                if let Some(header_bb) = target {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(header_bb, vec![]));
                    self.current_block = self.func.new_block();
                }
            }

            Stmt::Match { expr, arms, .. } => {
                let scrutinee = self.lower_expr(expr);
                let merge_bb = self.func.new_block();

                // build chain: for each arm, compare → arm body or next check
                let mut arm_blocks: Vec<(Block, Block)> = Vec::new(); // (check_bb, body_bb)
                for _ in arms {
                    let check = self.func.new_block();
                    let body = self.func.new_block();
                    arm_blocks.push((check, body));
                }

                // jump to first check
                if let Some(&(first_check, _)) = arm_blocks.first() {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(first_check, vec![]));
                } else {
                    self.func
                        .set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
                }

                for (i, arm) in arms.iter().enumerate() {
                    let (check_bb, body_bb) = arm_blocks[i];
                    let next = if i + 1 < arm_blocks.len() {
                        arm_blocks[i + 1].0
                    } else {
                        merge_bb // fallthrough if no match (should have wildcard)
                    };

                    // check block: compare scrutinee to pattern
                    self.current_block = check_bb;
                    match &arm.pattern {
                        ast::Pattern::IntLit(v) => {
                            let pat_val = self.emit(Op::ConstI32(*v as i32), IrType::I32);
                            let cond = self.emit(Op::Eq(scrutinee, pat_val), IrType::Bool);
                            self.func.set_terminator(
                                self.current_block,
                                Terminator::BranchIf {
                                    cond,
                                    then_block: body_bb,
                                    then_args: vec![],
                                    else_block: next,
                                    else_args: vec![],
                                },
                            );
                        }
                        ast::Pattern::Wildcard | ast::Pattern::Ident(_) => {
                            // wildcard or binding: always matches
                            if let ast::Pattern::Ident(name) = &arm.pattern {
                                self.vars.insert(name.clone(), scrutinee);
                            }
                            self.func.set_terminator(
                                self.current_block,
                                Terminator::Jump(body_bb, vec![]),
                            );
                        }
                        ast::Pattern::Variant(_, _) => {
                            // TODO: enum variant matching
                            self.func.set_terminator(
                                self.current_block,
                                Terminator::Jump(body_bb, vec![]),
                            );
                        }
                    }

                    // body block
                    self.current_block = body_bb;
                    self.lower_expr(&arm.body);
                    if matches!(
                        self.func.blocks[self.current_block.0 as usize].terminator,
                        Terminator::None
                    ) {
                        self.func
                            .set_terminator(self.current_block, Terminator::Jump(merge_bb, vec![]));
                    }
                }

                self.current_block = merge_bb;
            }

            _ => {} // defer, critical_section — TODO
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::IntLit(v, _) => self.emit(Op::ConstI32(*v as i32), IrType::I32),

            Expr::BoolLit(v, _) => self.emit(Op::ConstBool(*v), IrType::Bool),

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
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        IrType::Bool
                    }
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

            Expr::Field(obj, field, _) => {
                // check if this is a peripheral field access (b.gpio) — skip offset calc
                if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if let Some(VarKind::Board(_)) = self.var_kinds.get(var_name) {
                        return self.lower_expr(obj);
                    }
                }
                // struct field access: base_ptr + offset → load
                let base = self.lower_expr(obj);
                if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if let Some(struct_name) = self.var_types.get(var_name) {
                        if let Some(layout) = self.struct_layouts.get(struct_name) {
                            if let Some((offset, ty)) = layout.field_offset(field) {
                                if offset == 0 {
                                    return self.emit(Op::Load(base, ty), ty);
                                }
                                let off = self.emit(Op::ConstI32(offset as i32), IrType::I32);
                                let addr = self.emit(Op::Add(base, off), IrType::Ptr);
                                return self.emit(Op::Load(addr, ty), ty);
                            }
                        }
                    }
                }
                base
            }

            Expr::StringLit(s, _) => {
                // find the string's label in the global table
                let label = self
                    .globals
                    .strings
                    .iter()
                    .find(|(_, data)| data == s.as_bytes())
                    .map(|(l, _)| l.clone())
                    .unwrap_or_else(|| ".str0".into());
                self.emit(Op::GlobalAddr(label), IrType::Ptr)
            }

            Expr::StructLit(name, fields, _) => {
                if let Some(layout) = self.struct_layouts.get(name) {
                    // allocate stack space for the struct
                    let base = self.emit(Op::StackAlloc(layout.size), IrType::Ptr);
                    // store each field at its offset
                    for (field_name, expr) in fields {
                        let val = self.lower_expr(expr);
                        if let Some((offset, _ty)) = layout.field_offset(field_name) {
                            if offset == 0 {
                                self.emit(Op::Store(base, val), IrType::Void);
                            } else {
                                let off = self.emit(Op::ConstI32(offset as i32), IrType::I32);
                                let addr = self.emit(Op::Add(base, off), IrType::Ptr);
                                self.emit(Op::Store(addr, val), IrType::Void);
                            }
                        }
                    }
                    base
                } else {
                    self.emit(Op::Nop, IrType::Void)
                }
            }

            Expr::ArrayLit(elements, _) => {
                if elements.is_empty() {
                    return self.emit(Op::Nop, IrType::Void);
                }
                // allocate stack space: 4 bytes per element (assume u32 for now)
                let elem_size = 4u32;
                let total = elem_size * elements.len() as u32;
                let base = self.emit(Op::StackAlloc(total), IrType::Ptr);
                for (i, elem) in elements.iter().enumerate() {
                    let val = self.lower_expr(elem);
                    if i == 0 {
                        self.emit(Op::Store(base, val), IrType::Void);
                    } else {
                        let off =
                            self.emit(Op::ConstI32((i as u32 * elem_size) as i32), IrType::I32);
                        let addr = self.emit(Op::Add(base, off), IrType::Ptr);
                        self.emit(Op::Store(addr, val), IrType::Void);
                    }
                }
                base
            }

            Expr::Index(array, index, _) => {
                let base = self.lower_expr(array);
                let idx = self.lower_expr(index);
                let elem_size = 4u32; // assume u32 elements
                let size = self.emit(Op::ConstI32(elem_size as i32), IrType::I32);
                let offset = self.emit(Op::Mul(idx, size), IrType::I32);
                let addr = self.emit(Op::Add(base, offset), IrType::Ptr);
                self.emit(Op::Load(addr, IrType::I32), IrType::I32)
            }

            Expr::Try(inner, _) => {
                // evaluate the inner expression (should be a function call returning !T)
                let payload = self.lower_expr(inner);
                // get the error tag (a1 after the call)
                let tag = self.emit(Op::GetErrorTag, IrType::I32);
                // branch: if tag != 0, propagate error
                let zero = self.emit(Op::ConstI32(0), IrType::I32);
                let is_err = self.emit(Op::Ne(tag, zero), IrType::Bool);
                let err_bb = self.func.new_block();
                let ok_bb = self.func.new_block();
                self.func.set_terminator(
                    self.current_block,
                    Terminator::BranchIf {
                        cond: is_err,
                        then_block: err_bb,
                        then_args: vec![],
                        else_block: ok_bb,
                        else_args: vec![],
                    },
                );
                // error path: propagate
                self.current_block = err_bb;
                self.func
                    .set_terminator(self.current_block, Terminator::ReturnError(payload, tag));
                // ok path: unwrap payload
                self.current_block = ok_bb;
                payload
            }

            _ => {
                // DotEnum, Cast, etc — TODO
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
        assert!(
            has_block_param,
            "for-loop should create block parameter for loop var"
        );
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
            b.insts
                .iter()
                .any(|i| matches!(&i.op, Op::GlobalAddr(n) if n == "counter"))
        });
        assert!(
            has_global_addr,
            "should emit GlobalAddr for global variable read"
        );
        let has_load = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::Load(_, _))));
        assert!(has_load, "should emit Load after GlobalAddr");
        println!("{}", func);
    }

    #[test]
    fn lower_global_write() {
        let ir = lower("static mut counter: u32 = 0;\nfn set() { counter = 42; }");
        assert_eq!(ir.functions.len(), 1);
        let func = &ir.functions[0];
        let has_store = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::Store(_, _))));
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
        assert!(
            func.blocks.len() >= 3,
            "while with continue should have cond/body/exit blocks"
        );
        println!("{}", func);
    }

    #[test]
    fn lower_global_increment_with_break() {
        let ir = lower(
            "static mut ticks: u32 = 0;\nfn f() { loop { ticks = ticks + 1; if ticks == 10 { break; } } }",
        );
        let func = &ir.functions[0];
        // should have GlobalAddr, Load, Store ops and a break jump
        let has_global = func.blocks.iter().any(|b| {
            b.insts
                .iter()
                .any(|i| matches!(&i.op, Op::GlobalAddr(n) if n == "ticks"))
        });
        assert!(has_global, "should reference 'ticks' global");
        println!("{}", func);
    }

    #[test]
    fn lower_match_int_patterns() {
        let ir = lower("fn f(x: u32) u32 { match x { 0 => 10, 1 => 20, _ => 30, } }");
        let func = &ir.functions[0];
        // 3 arms = 3 check blocks + 3 body blocks + entry + merge = at least 8
        assert!(
            func.blocks.len() >= 7,
            "match with 3 arms needs multiple blocks, got {}",
            func.blocks.len()
        );
        // should have Eq comparisons for int patterns
        let eq_count = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(&i.op, Op::Eq(_, _)))
            .count();
        assert!(
            eq_count >= 2,
            "should have at least 2 Eq comparisons for int patterns"
        );
        println!("{}", func);
    }

    #[test]
    fn lower_match_wildcard_only() {
        let ir = lower("fn f(x: u32) u32 { match x { _ => 42, } }");
        let func = &ir.functions[0];
        assert!(
            func.blocks.len() >= 3,
            "match with wildcard needs check+body+merge"
        );
        println!("{}", func);
    }

    #[test]
    fn lower_struct_lit_and_field() {
        let ir = lower(
            "struct Point { x: u32, y: u32 }\nfn f() u32 { let p = Point { x: 10, y: 20 }; return p.x; }",
        );
        let func = &ir.functions[0];
        // should have StackAlloc for the struct
        let has_alloc = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::StackAlloc(_))));
        assert!(has_alloc, "struct literal should emit StackAlloc");
        // should have Store ops for field initialization
        let store_count = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(&i.op, Op::Store(_, _)))
            .count();
        assert!(
            store_count >= 2,
            "should store both fields, got {}",
            store_count
        );
        // should have Load for field read
        let has_load = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::Load(_, _))));
        assert!(has_load, "field access should emit Load");
        println!("{}", func);
    }

    #[test]
    fn struct_layout_offsets() {
        let layout = StructLayout::from_def(&ast::StructDef {
            name: "Test".into(),
            type_params: Vec::new(),
            fields: vec![
                ast::StructField {
                    name: "a".into(),
                    ty: ast::Type::Primitive(ast::PrimitiveType::U8),
                    span: crate::lexer::token::Span { start: 0, end: 0 },
                },
                ast::StructField {
                    name: "b".into(),
                    ty: ast::Type::Primitive(ast::PrimitiveType::U32),
                    span: crate::lexer::token::Span { start: 0, end: 0 },
                },
                ast::StructField {
                    name: "c".into(),
                    ty: ast::Type::Primitive(ast::PrimitiveType::U16),
                    span: crate::lexer::token::Span { start: 0, end: 0 },
                },
            ],
            span: crate::lexer::token::Span { start: 0, end: 0 },
        });
        // u8 at 0, u32 aligned to 4, u16 aligned to 2 after u32
        assert_eq!(layout.field_offset("a"), Some((0, IrType::I8)));
        assert_eq!(layout.field_offset("b"), Some((4, IrType::I32)));
        assert_eq!(layout.field_offset("c"), Some((8, IrType::I16)));
        assert_eq!(layout.size, 12); // 10 rounded to 4
    }

    #[test]
    fn lower_array_lit_and_index() {
        let ir = lower("fn f() u32 { let arr = [10, 20, 30]; return arr[1]; }");
        let func = &ir.functions[0];
        // should have StackAlloc for the array
        let has_alloc = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::StackAlloc(_))));
        assert!(has_alloc, "array literal should emit StackAlloc");
        // should have Stores for each element
        let store_count = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(&i.op, Op::Store(_, _)))
            .count();
        assert!(
            store_count >= 3,
            "should store 3 elements, got {}",
            store_count
        );
        // should have Mul for index offset calculation
        let has_mul = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::Mul(_, _))));
        assert!(has_mul, "index should emit Mul for offset");
        println!("{}", func);
    }

    #[test]
    fn lower_string_literal() {
        let ir = lower("fn f() { let s = \"hello\"; }");
        // string should be collected into globals
        assert!(
            !ir.globals.strings.is_empty(),
            "string literal should be in global table"
        );
        assert_eq!(ir.globals.strings[0].1, b"hello");
        // function should emit GlobalAddr for the string
        let func = &ir.functions[0];
        let has_global_addr = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::GlobalAddr(_))));
        assert!(has_global_addr, "string should emit GlobalAddr");
        println!("{}", func);
    }

    #[test]
    fn lower_try_expression() {
        let ir = lower(
            "fn read_sensor() !u32 { return 42; }\nfn f() !u32 { let x = try read_sensor(); return x; }",
        );
        assert_eq!(ir.functions.len(), 2);
        let func = &ir.functions[1]; // f
        // should have GetErrorTag op
        let has_get_tag = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(&i.op, Op::GetErrorTag)));
        assert!(has_get_tag, "try should emit GetErrorTag");
        // should have a ReturnError terminator in one of the blocks
        let has_return_error = func
            .blocks
            .iter()
            .any(|b| matches!(b.terminator, Terminator::ReturnError(_, _)));
        assert!(
            has_return_error,
            "try should emit ReturnError for error propagation"
        );
        println!("{}", func);
    }

    #[test]
    fn lower_labeled_break() {
        let ir = lower("fn f() { 'outer: loop { loop { break 'outer; } } }");
        let func = &ir.functions[0];
        // the inner break should jump to outer's exit, not inner's exit
        // should have at least 4 blocks: entry, outer_loop, inner_loop, outer_exit
        assert!(
            func.blocks.len() >= 4,
            "labeled break needs outer+inner loops + exit"
        );
        println!("{}", func);
    }
}
