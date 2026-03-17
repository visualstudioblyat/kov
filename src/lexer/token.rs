/// Source location — byte offset range in the source file.
/// Used for error reporting with exact spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }
}

/// A token with its kind and source location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Every possible token the lexer can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Literals ──
    IntLit(u64),
    FloatLit(f64),
    StringLit(String),
    CharLit(char),

    // ── Identifier ──
    Ident(String),
    Lifetime(String), // 'label

    // ── Keywords ──
    Board,
    Extern,
    Fn,
    Trait,
    Impl,
    Struct,
    Enum,
    Const,
    Static,
    Type,
    Import,
    Interrupt,
    Let,
    Mut,
    If,
    Else,
    Loop,
    For,
    In,
    While,
    Match,
    Return,
    Break,
    Continue,
    Defer,
    Try,
    As,
    True,
    False,
    Void,
    CriticalSection,

    // ── Type keywords ──
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

    // ── Register type keywords ──
    Reg,
    RO,
    WO,
    RW,
    Shared,
    Buffer,
    Fixed,

    // ── Punctuation ──
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    Semicolon,  // ;
    Colon,      // :
    ColonColon, // ::
    Comma,      // ,
    Dot,        // .
    DotDot,     // ..
    Arrow,      // ->
    FatArrow,   // =>
    At,         // @
    Hash,       // #
    Underscore, // _

    // ── Operators ──
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Percent, // %
    Amp,     // &
    Pipe,    // |
    Caret,   // ^
    Tilde,   // ~
    Bang,    // !
    Shl,     // <<
    Shr,     // >>

    // ── Comparison ──
    Eq, // ==
    Ne, // !=
    Lt, // <
    Gt, // >
    Le, // <=
    Ge, // >=

    // ── Logical ──
    AmpAmp,   // &&
    PipePipe, // ||

    // ── Assignment ──
    Assign,    // =
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    AmpEq,     // &=
    PipeEq,    // |=
    CaretEq,   // ^=
    ShlEq,     // <<=
    ShrEq,     // >>=

    // ── Overflow operators ──
    WrapPlus,   // %+
    WrapMinus,  // %-
    WrapStar,   // %*
    SatPlus,    // sat+
    SatMinus,   // sat-
    CheckPlus,  // ?+
    CheckMinus, // ?-

    // ── Special ──
    Eof,
}

impl TokenKind {
    /// Look up a keyword from an identifier string.
    /// Returns None if it's not a keyword (it's a regular identifier).
    pub fn keyword(s: &str) -> Option<TokenKind> {
        match s {
            "board" => Some(TokenKind::Board),
            "extern" => Some(TokenKind::Extern),
            "fn" => Some(TokenKind::Fn),
            "trait" => Some(TokenKind::Trait),
            "impl" => Some(TokenKind::Impl),
            "struct" => Some(TokenKind::Struct),
            "enum" => Some(TokenKind::Enum),
            "const" => Some(TokenKind::Const),
            "static" => Some(TokenKind::Static),
            "type" => Some(TokenKind::Type),
            "import" => Some(TokenKind::Import),
            "interrupt" => Some(TokenKind::Interrupt),
            "let" => Some(TokenKind::Let),
            "mut" => Some(TokenKind::Mut),
            "if" => Some(TokenKind::If),
            "else" => Some(TokenKind::Else),
            "loop" => Some(TokenKind::Loop),
            "for" => Some(TokenKind::For),
            "in" => Some(TokenKind::In),
            "while" => Some(TokenKind::While),
            "match" => Some(TokenKind::Match),
            "return" => Some(TokenKind::Return),
            "break" => Some(TokenKind::Break),
            "continue" => Some(TokenKind::Continue),
            "defer" => Some(TokenKind::Defer),
            "try" => Some(TokenKind::Try),
            "as" => Some(TokenKind::As),
            "true" => Some(TokenKind::True),
            "false" => Some(TokenKind::False),
            "void" => Some(TokenKind::Void),
            "critical_section" => Some(TokenKind::CriticalSection),
            "u8" => Some(TokenKind::U8),
            "u16" => Some(TokenKind::U16),
            "u32" => Some(TokenKind::U32),
            "u64" => Some(TokenKind::U64),
            "i8" => Some(TokenKind::I8),
            "i16" => Some(TokenKind::I16),
            "i32" => Some(TokenKind::I32),
            "i64" => Some(TokenKind::I64),
            "bool" => Some(TokenKind::Bool),
            "usize" => Some(TokenKind::Usize),
            "isize" => Some(TokenKind::Isize),
            "Reg" => Some(TokenKind::Reg),
            "RO" => Some(TokenKind::RO),
            "WO" => Some(TokenKind::WO),
            "RW" => Some(TokenKind::RW),
            "Shared" => Some(TokenKind::Shared),
            "Buffer" => Some(TokenKind::Buffer),
            "Fixed" => Some(TokenKind::Fixed),
            _ => None,
        }
    }
}
