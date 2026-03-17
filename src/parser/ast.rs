use crate::lexer::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<TopItem>,
}

#[derive(Debug, Clone)]
pub enum TopItem {
    Import(ImportDecl),
    Board(BoardDef),
    Function(FnDef),
    Interrupt(InterruptDef),
    Struct(StructDef),
    Enum(EnumDef),
    Const(ConstDef),
    Static(StaticDef),
    TypeAlias(TypeAlias),
    ExternFn(ExternFnDecl),
    Trait(TraitDef),
    Impl(ImplBlock),
    ConstAssert(Expr, Span),
}

#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    pub has_default: bool,
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub trait_name: Option<String>,
    pub target_type: String,
    pub methods: Vec<FnDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExternFnDecl {
    pub abi: String,
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BoardDef {
    pub name: String,
    pub fields: Vec<BoardField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BoardField {
    pub name: String,
    pub ty: Type,
    pub address: Option<Expr>, // @ 0x6000_4000
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub attrs: Vec<Attribute>,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    pub is_error_return: bool,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeParam {
    pub name: String,
    pub bounds: Vec<String>, // T: Copy + Sized
}

#[derive(Debug, Clone)]
pub struct InterruptDef {
    pub interrupt_name: String,
    pub priority: Option<u64>,
    pub fn_name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StaticDef {
    pub name: String,
    pub mutable: bool,
    pub ty: Type,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

// ── types ──

#[derive(Debug, Clone)]
pub enum Type {
    Primitive(PrimitiveType),
    Named(String, Vec<Type>),
    Array(Box<Type>, Box<Expr>),    // [T; N]
    Slice(Box<Type>),               // &[T]
    Ref(Box<Type>, bool),           // &T / &mut T
    Ptr(Box<Type>, Option<Region>), // *@flash T
    Register(Box<Type>, RegAccess), // Reg<u32, RW>
    Shared(Box<Type>),
    Buffer(String),        // Buffer<State>
    Fixed(u64, u64),       // Fixed<I, F>
    ErrorUnion(Box<Type>), // !T
    FnType(Vec<Type>, Option<Box<Type>>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrimitiveType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Bool,
    Usize,
    Isize,
    Void,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegAccess {
    RO,
    WO,
    RW,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Region {
    Flash,
    Ram,
    Mmio,
}

// ── statements ──

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail_expr: Option<Box<Expr>>, // last expr without ; = implicit return
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Type>,
        value: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
    Return(Option<Expr>, Span),
    Break(Option<String>, Span),
    Continue(Option<String>, Span),
    Defer(Box<Stmt>, Span),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<ElseBranch>,
        span: Span,
    },
    Loop(Option<String>, Block, Span),
    While {
        label: Option<String>,
        condition: Expr,
        body: Block,
        span: Span,
    },
    For {
        label: Option<String>,
        var: String,
        start: Expr,
        end: Expr,
        bound: Option<u64>,
        body: Block,
        span: Span,
    },
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        span: Span,
    },
    CriticalSection {
        token_name: String,
        body: Block,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    ElseIf(Box<Stmt>),
    Else(Block),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignOp {
    Assign,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Ident(String),
    Variant(String, Vec<String>),
    IntLit(u64),
    Wildcard,
}

// ── expressions ──

#[derive(Debug, Clone)]
pub enum Expr {
    IntLit(u64, Span),
    FloatLit(f64, Span),
    StringLit(String, Span),
    CharLit(char, Span),
    BoolLit(bool, Span),
    Ident(String, Span),
    Binary(Box<Expr>, BinOp, Box<Expr>, Span),
    Unary(UnaryOp, Box<Expr>, Span),
    Field(Box<Expr>, String, Span),
    MethodCall(Box<Expr>, String, Vec<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Cast(Box<Expr>, Type, Span),
    Try(Box<Expr>, Span),
    ArrayLit(Vec<Expr>, Span),
    StructLit(String, Vec<(String, Expr)>, Span),
    DotEnum(String, Span), // .output shorthand
    Block(Block),
    If(Box<Expr>, Block, Option<Block>, Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, s)
            | Expr::FloatLit(_, s)
            | Expr::StringLit(_, s)
            | Expr::CharLit(_, s)
            | Expr::BoolLit(_, s)
            | Expr::Ident(_, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Unary(_, _, s)
            | Expr::Field(_, _, s)
            | Expr::MethodCall(_, _, _, s)
            | Expr::Call(_, _, s)
            | Expr::Index(_, _, s)
            | Expr::Cast(_, _, s)
            | Expr::Try(_, s)
            | Expr::ArrayLit(_, s)
            | Expr::StructLit(_, _, s)
            | Expr::DotEnum(_, s)
            | Expr::If(_, _, _, s) => *s,
            Expr::Block(b) => b.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    WrapAdd,
    WrapSub,
    WrapMul,
    CheckAdd,
    CheckSub,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Ref,
    RefMut,
    Deref,
}
