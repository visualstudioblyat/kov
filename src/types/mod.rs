pub mod check;
pub mod interrupt;

use crate::lexer::token::Span;
use crate::parser::ast;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Bool,
    Void,
    Ptr(Box<Ty>),
    MutRef(Box<Ty>),
    Ref(Box<Ty>),
    Array(Box<Ty>, u64),
    Slice(Box<Ty>),
    // peripheral — tracks ownership, wraps the underlying register type
    Peripheral(String),
    // board type — the &mut board param
    Board(String),
    // named user type
    Named(String),
    // error union
    Error(Box<Ty>),
    Unknown,
}

impl Ty {
    pub fn from_ast(t: &ast::Type) -> Self {
        match t {
            ast::Type::Primitive(p) => match p {
                ast::PrimitiveType::U8 => Ty::U8,
                ast::PrimitiveType::U16 => Ty::U16,
                ast::PrimitiveType::U32 => Ty::U32,
                ast::PrimitiveType::U64 => Ty::U64,
                ast::PrimitiveType::I8 => Ty::I8,
                ast::PrimitiveType::I16 => Ty::I16,
                ast::PrimitiveType::I32 => Ty::I32,
                ast::PrimitiveType::I64 => Ty::I64,
                ast::PrimitiveType::Bool => Ty::Bool,
                ast::PrimitiveType::Usize => Ty::U32,
                ast::PrimitiveType::Isize => Ty::I32,
                ast::PrimitiveType::Void => Ty::Void,
            },
            ast::Type::Named(name, _) => Ty::Named(name.clone()),
            ast::Type::Ref(inner, false) => Ty::Ref(Box::new(Ty::from_ast(inner))),
            ast::Type::Ref(inner, true) => Ty::MutRef(Box::new(Ty::from_ast(inner))),
            ast::Type::Array(inner, _) => Ty::Array(Box::new(Ty::from_ast(inner)), 0),
            ast::Type::Slice(inner) => Ty::Slice(Box::new(Ty::from_ast(inner))),
            ast::Type::ErrorUnion(inner) => Ty::Error(Box::new(Ty::from_ast(inner))),
            _ => Ty::Unknown,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64
        )
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer()
    }
}

#[derive(Debug)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "type error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}
