pub mod globals;
pub mod lower;
pub mod types;

use types::IrType;

// value and block handles are indices into their respective pools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Block(pub u32);

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, IrType)>,
    pub ret_type: IrType,
    pub blocks: Vec<BasicBlock>,
    pub values: Vec<ValueDef>,
}

#[derive(Debug)]
pub struct BasicBlock {
    pub params: Vec<(Value, IrType)>, // block parameters (SSA phi replacement)
    pub insts: Vec<Inst>,
    pub terminator: Terminator,
}

#[derive(Debug)]
pub struct ValueDef {
    pub ty: IrType,
    pub name: Option<String>, // debug name from source
}

// every instruction produces a Value (except void ops which produce Value::VOID)
#[derive(Debug)]
pub struct Inst {
    pub result: Value,
    pub op: Op,
}

#[derive(Debug)]
pub enum Op {
    // constants
    ConstI32(i32),
    ConstI64(i64),
    ConstBool(bool),

    // arithmetic (all i32 for rv32)
    Add(Value, Value),
    Sub(Value, Value),
    Mul(Value, Value),
    Div(Value, Value),
    Rem(Value, Value),

    // bitwise
    And(Value, Value),
    Or(Value, Value),
    Xor(Value, Value),
    Shl(Value, Value),
    Shr(Value, Value), // logical
    Sar(Value, Value), // arithmetic

    // comparison → bool
    Eq(Value, Value),
    Ne(Value, Value),
    Lt(Value, Value),
    Ge(Value, Value),
    Ltu(Value, Value), // unsigned
    Geu(Value, Value),

    // unary
    Neg(Value),
    Not(Value), // bitwise not

    // memory
    Load(Value, IrType), // load from address
    Store(Value, Value), // store(addr, val)

    // volatile MMIO — never optimized away
    VolatileLoad(Value, IrType),
    VolatileStore(Value, Value),

    // function call
    Call(String, Vec<Value>),

    // zero-extend / sign-extend / truncate
    Zext(Value, IrType),
    Sext(Value, IrType),
    Trunc(Value, IrType),

    // stack allocation (returns pointer)
    StackAlloc(u32), // size in bytes

    // get address of global/static
    GlobalAddr(String),

    // no-op (used for void expressions)
    Nop,
}

#[derive(Debug)]
pub enum Terminator {
    // unconditional jump with block args
    Jump(Block, Vec<Value>),

    // conditional branch
    BranchIf {
        cond: Value,
        then_block: Block,
        then_args: Vec<Value>,
        else_block: Block,
        else_args: Vec<Value>,
    },

    // return from function
    Return(Option<Value>),

    // unreachable (after diverging calls, infinite loops)
    Unreachable,

    // placeholder during construction
    None,
}

impl Function {
    pub fn new(name: String, params: Vec<(String, IrType)>, ret_type: IrType) -> Self {
        Self {
            name,
            params,
            ret_type,
            blocks: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn new_block(&mut self) -> Block {
        let id = Block(self.blocks.len() as u32);
        self.blocks.push(BasicBlock {
            params: Vec::new(),
            insts: Vec::new(),
            terminator: Terminator::None,
        });
        id
    }

    pub fn new_value(&mut self, ty: IrType, name: Option<String>) -> Value {
        let id = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty, name });
        id
    }

    pub fn push_inst(&mut self, block: Block, op: Op, ty: IrType) -> Value {
        let val = self.new_value(ty, None);
        self.blocks[block.0 as usize]
            .insts
            .push(Inst { result: val, op });
        val
    }

    pub fn set_terminator(&mut self, block: Block, term: Terminator) {
        self.blocks[block.0 as usize].terminator = term;
    }

    pub fn add_block_param(&mut self, block: Block, ty: IrType) -> Value {
        let val = self.new_value(ty, None);
        self.blocks[block.0 as usize].params.push((val, ty));
        val
    }
}

// pretty printer for debugging
impl std::fmt::Display for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fn {}(", self.name)?;
        for (i, (name, ty)) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {:?}", name, ty)?;
        }
        writeln!(f, ") -> {:?} {{", self.ret_type)?;

        for (bi, block) in self.blocks.iter().enumerate() {
            write!(f, "  b{}(", bi)?;
            for (i, (val, ty)) in block.params.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "v{}: {:?}", val.0, ty)?;
            }
            writeln!(f, "):")?;

            for inst in &block.insts {
                writeln!(f, "    v{} = {:?}", inst.result.0, inst.op)?;
            }
            writeln!(f, "    {:?}", block.terminator)?;
        }
        write!(f, "}}")
    }
}
