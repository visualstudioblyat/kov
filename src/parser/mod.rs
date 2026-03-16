pub mod ast;

use crate::lexer::token::{Span, Token, TokenKind};
use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parse error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ParseError {}

type R<T> = Result<T, ParseError>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<Program, Vec<ParseError>> {
        let mut items = Vec::new();
        let mut errors = Vec::new();
        while !self.at_eof() {
            match self.parse_top_item() {
                Ok(item) => items.push(item),
                Err(e) => {
                    errors.push(e);
                    // synchronize: skip tokens until we find a top-level keyword
                    self.synchronize();
                }
            }
        }
        if errors.is_empty() {
            Ok(Program { items })
        } else {
            Err(errors)
        }
    }

    fn synchronize(&mut self) {
        while !self.at_eof() {
            match self.peek() {
                TokenKind::Fn
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Board
                | TokenKind::Const
                | TokenKind::Static
                | TokenKind::Import
                | TokenKind::Interrupt
                | TokenKind::Type
                | TokenKind::Extern => return,
                _ => {
                    self.advance_any();
                }
            }
        }
    }

    // ── token access ──

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::new(0, 0))
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    fn advance_any(&mut self) {
        if !self.at_eof() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: &TokenKind) -> R<Span> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            Ok(self.advance().span)
        } else {
            Err(self.error(format!("expected {:?}, got {:?}", expected, self.peek())))
        }
    }

    fn expect_ident(&mut self) -> R<String> {
        if let TokenKind::Ident(s) = self.peek().clone() {
            self.advance();
            Ok(s)
        } else {
            Err(self.error(format!("expected identifier, got {:?}", self.peek())))
        }
    }

    // keywords are valid path segments (e.g. import board::esp32c3)
    fn expect_ident_or_keyword(&mut self) -> R<String> {
        if let TokenKind::Ident(s) = self.peek().clone() {
            self.advance();
            return Ok(s);
        }
        // treat keywords as identifiers in path position
        let name = match self.peek() {
            TokenKind::Board => "board",
            TokenKind::Type => "type",
            _ => return Err(self.error(format!("expected identifier, got {:?}", self.peek()))),
        };
        self.advance();
        Ok(name.to_string())
    }

    fn expect_int(&mut self) -> R<u64> {
        if let TokenKind::IntLit(v) = self.peek() {
            let v = *v;
            self.advance();
            Ok(v)
        } else {
            Err(self.error(format!("expected integer, got {:?}", self.peek())))
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error(&self, message: String) -> ParseError {
        ParseError {
            message,
            span: self.span(),
        }
    }

    // ── top-level items ──

    fn parse_top_item(&mut self) -> R<TopItem> {
        // collect attributes before the item
        let attrs = self.parse_attributes()?;

        match self.peek() {
            TokenKind::Import => Ok(TopItem::Import(self.parse_import()?)),
            TokenKind::Board => Ok(TopItem::Board(self.parse_board()?)),
            TokenKind::Fn => Ok(TopItem::Function(self.parse_fn(attrs)?)),
            TokenKind::Interrupt => Ok(TopItem::Interrupt(self.parse_interrupt()?)),
            TokenKind::Struct => Ok(TopItem::Struct(self.parse_struct()?)),
            TokenKind::Enum => Ok(TopItem::Enum(self.parse_enum()?)),
            TokenKind::Const => Ok(TopItem::Const(self.parse_const()?)),
            TokenKind::Static => Ok(TopItem::Static(self.parse_static()?)),
            TokenKind::Type => Ok(TopItem::TypeAlias(self.parse_type_alias()?)),
            TokenKind::Extern => Ok(TopItem::ExternFn(self.parse_extern_fn()?)),
            _ => Err(self.error(format!("unexpected token {:?} at top level", self.peek()))),
        }
    }

    fn parse_attributes(&mut self) -> R<Vec<Attribute>> {
        let mut attrs = Vec::new();
        while matches!(self.peek(), TokenKind::Hash) {
            let start = self.span();
            self.advance(); // #
            self.expect(&TokenKind::LBracket)?;
            let name = self.expect_ident()?;
            let mut args = Vec::new();
            if self.eat(&TokenKind::LParen) {
                if !matches!(self.peek(), TokenKind::RParen) {
                    args.push(self.parse_expr()?);
                    while self.eat(&TokenKind::Comma) {
                        args.push(self.parse_expr()?);
                    }
                }
                self.expect(&TokenKind::RParen)?;
            }
            let end = self.expect(&TokenKind::RBracket)?;
            attrs.push(Attribute {
                name,
                args,
                span: Span::new(start.start, end.end),
            });
        }
        Ok(attrs)
    }

    fn parse_import(&mut self) -> R<ImportDecl> {
        let start = self.span();
        self.advance(); // import
        let mut path = vec![self.expect_ident_or_keyword()?];
        while self.eat(&TokenKind::ColonColon) {
            path.push(self.expect_ident_or_keyword()?);
        }
        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(ImportDecl {
            path,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_board(&mut self) -> R<BoardDef> {
        let start = self.span();
        self.advance(); // board
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            fields.push(self.parse_board_field()?);
        }
        let end = self.expect(&TokenKind::RBrace)?;
        Ok(BoardDef {
            name,
            fields,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_board_field(&mut self) -> R<BoardField> {
        let start = self.span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;

        // could be Type @ address, or just a bare value (like clock: 160_000_000)
        if matches!(self.peek(), TokenKind::IntLit(_)) {
            let value = self.parse_expr()?;
            let end = self.span();
            self.eat(&TokenKind::Comma);
            return Ok(BoardField {
                name,
                ty: Type::Primitive(PrimitiveType::U32),
                address: Some(value),
                span: Span::new(start.start, end.end),
            });
        }

        let ty = self.parse_type()?;
        let address = if self.eat(&TokenKind::At) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let end = self.span();
        self.eat(&TokenKind::Comma);
        Ok(BoardField {
            name,
            ty,
            address,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_fn(&mut self, attrs: Vec<Attribute>) -> R<FnDef> {
        let start = self.span();
        self.advance(); // fn
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::RParen)?;

        let mut is_error_return = false;
        let ret_type = if !matches!(self.peek(), TokenKind::LBrace) {
            if self.eat(&TokenKind::Bang) {
                is_error_return = true;
            }
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        let end_span = body.span;
        Ok(FnDef {
            name,
            type_params,
            attrs,
            params,
            ret_type,
            is_error_return,
            body,
            span: Span::new(start.start, end_span.end),
        })
    }

    fn parse_interrupt(&mut self) -> R<InterruptDef> {
        let start = self.span();
        self.advance(); // interrupt
        self.expect(&TokenKind::LParen)?;
        let interrupt_name = self.expect_ident()?;
        let priority = if self.eat(&TokenKind::Comma) {
            self.expect_ident()?; // "priority"
            self.expect(&TokenKind::Assign)?;
            Some(self.expect_int()?)
        } else {
            None
        };
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Fn)?;
        let fn_name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;
        let body = self.parse_block()?;
        let end_span = body.span;
        Ok(InterruptDef {
            interrupt_name,
            priority,
            fn_name,
            body,
            span: Span::new(start.start, end_span.end),
        })
    }

    fn parse_struct(&mut self) -> R<StructDef> {
        let start = self.span();
        self.advance(); // struct
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            let fs = self.span();
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_type()?;
            let fe = self.span();
            self.eat(&TokenKind::Comma);
            fields.push(StructField {
                name: fname,
                ty,
                span: Span::new(fs.start, fe.end),
            });
        }
        let end = self.expect(&TokenKind::RBrace)?;
        Ok(StructDef {
            name,
            type_params,
            fields,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_enum(&mut self) -> R<EnumDef> {
        let start = self.span();
        self.advance(); // enum
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            let vs = self.span();
            let vname = self.expect_ident()?;
            let mut fields = Vec::new();
            if self.eat(&TokenKind::LParen) {
                if !matches!(self.peek(), TokenKind::RParen) {
                    fields.push(self.parse_type()?);
                    while self.eat(&TokenKind::Comma) {
                        fields.push(self.parse_type()?);
                    }
                }
                self.expect(&TokenKind::RParen)?;
            }
            let ve = self.span();
            self.eat(&TokenKind::Comma);
            variants.push(EnumVariant {
                name: vname,
                fields,
                span: Span::new(vs.start, ve.end),
            });
        }
        let end = self.expect(&TokenKind::RBrace)?;
        Ok(EnumDef {
            name,
            variants,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_const(&mut self) -> R<ConstDef> {
        let start = self.span();
        self.advance(); // const
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_expr()?;
        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(ConstDef {
            name,
            ty,
            value,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_static(&mut self) -> R<StaticDef> {
        let start = self.span();
        self.advance(); // static
        let mutable = self.eat(&TokenKind::Mut);
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_expr()?;
        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(StaticDef {
            name,
            mutable,
            ty,
            value,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_type_alias(&mut self) -> R<TypeAlias> {
        let start = self.span();
        self.advance(); // type
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Assign)?;
        let ty = self.parse_type()?;
        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(TypeAlias {
            name,
            ty,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_extern_fn(&mut self) -> R<ExternFnDecl> {
        let start = self.span();
        self.advance(); // extern
        // parse ABI string: "C"
        let abi = if let TokenKind::StringLit(s) = self.peek().clone() {
            self.advance();
            s
        } else {
            "C".to_string()
        };
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::RParen)?;

        let ret_type = if !matches!(self.peek(), TokenKind::Semicolon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(ExternFnDecl {
            abi,
            name,
            params,
            ret_type,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_type_params(&mut self) -> R<Vec<TypeParam>> {
        if !self.eat(&TokenKind::Lt) {
            return Ok(Vec::new());
        }
        let mut params = Vec::new();
        loop {
            let name = self.expect_ident()?;
            let mut bounds = Vec::new();
            if self.eat(&TokenKind::Colon) {
                bounds.push(self.expect_ident()?);
                while self.eat(&TokenKind::Plus) {
                    bounds.push(self.expect_ident()?);
                }
            }
            params.push(TypeParam { name, bounds });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::Gt)?;
        Ok(params)
    }

    fn parse_param_list(&mut self) -> R<Vec<Param>> {
        let mut params = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            return Ok(params);
        }
        params.push(self.parse_param()?);
        while self.eat(&TokenKind::Comma) {
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> R<Param> {
        let start = self.span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        Ok(Param {
            name,
            ty,
            span: Span::new(start.start, self.span().start),
        })
    }

    // ── types ──

    fn parse_type(&mut self) -> R<Type> {
        match self.peek() {
            TokenKind::U8 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::U8))
            }
            TokenKind::U16 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::U16))
            }
            TokenKind::U32 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::U32))
            }
            TokenKind::U64 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::U64))
            }
            TokenKind::I8 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::I8))
            }
            TokenKind::I16 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::I16))
            }
            TokenKind::I32 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::I32))
            }
            TokenKind::I64 => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::I64))
            }
            TokenKind::Bool => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::Bool))
            }
            TokenKind::Usize => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::Usize))
            }
            TokenKind::Isize => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::Isize))
            }
            TokenKind::Void => {
                self.advance();
                Ok(Type::Primitive(PrimitiveType::Void))
            }

            TokenKind::Amp => {
                self.advance();
                // &[T] = slice, &T / &mut T = reference
                if matches!(self.peek(), TokenKind::LBracket) {
                    self.advance();
                    let inner = self.parse_type()?;
                    self.expect(&TokenKind::RBracket)?;
                    Ok(Type::Slice(Box::new(inner)))
                } else {
                    let is_mut = self.eat(&TokenKind::Mut);
                    let inner = self.parse_type()?;
                    Ok(Type::Ref(Box::new(inner), is_mut))
                }
            }

            TokenKind::Bang => {
                self.advance();
                let inner = self.parse_type()?;
                Ok(Type::ErrorUnion(Box::new(inner)))
            }

            TokenKind::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Semicolon)?;
                let size = self.parse_expr()?;
                self.expect(&TokenKind::RBracket)?;
                Ok(Type::Array(Box::new(inner), Box::new(size)))
            }

            TokenKind::Ident(_) => {
                let name = self.expect_ident()?;
                if self.eat(&TokenKind::Lt) {
                    let mut args = vec![self.parse_type()?];
                    while self.eat(&TokenKind::Comma) {
                        args.push(self.parse_type()?);
                    }
                    self.expect(&TokenKind::Gt)?;
                    Ok(Type::Named(name, args))
                } else {
                    Ok(Type::Named(name, vec![]))
                }
            }

            TokenKind::Fixed => {
                self.advance();
                self.expect(&TokenKind::Lt)?;
                let i = self.expect_int()?;
                self.expect(&TokenKind::Comma)?;
                let f = self.expect_int()?;
                self.expect(&TokenKind::Gt)?;
                Ok(Type::Fixed(i, f))
            }

            TokenKind::Reg => {
                self.advance();
                self.expect(&TokenKind::Lt)?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Comma)?;
                let access = match self.peek() {
                    TokenKind::RO => {
                        self.advance();
                        RegAccess::RO
                    }
                    TokenKind::WO => {
                        self.advance();
                        RegAccess::WO
                    }
                    TokenKind::RW => {
                        self.advance();
                        RegAccess::RW
                    }
                    _ => return Err(self.error("expected RO, WO, or RW".into())),
                };
                self.expect(&TokenKind::Gt)?;
                Ok(Type::Register(Box::new(inner), access))
            }

            TokenKind::Shared => {
                self.advance();
                self.expect(&TokenKind::Lt)?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Gt)?;
                Ok(Type::Shared(Box::new(inner)))
            }

            TokenKind::Buffer => {
                self.advance();
                self.expect(&TokenKind::Lt)?;
                let state = self.expect_ident()?;
                self.expect(&TokenKind::Gt)?;
                Ok(Type::Buffer(state))
            }

            _ => Err(self.error(format!("expected type, got {:?}", self.peek()))),
        }
    }

    // ── block and statements ──

    fn parse_block(&mut self) -> R<Block> {
        let start = self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();

        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
        }

        let end = self.expect(&TokenKind::RBrace)?;

        // check if last stmt is an expression without semicolon → tail expr
        // for now, no tail expr detection (all stmts end with ;)
        Ok(Block {
            stmts,
            tail_expr: None,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_stmt(&mut self) -> R<Stmt> {
        match self.peek() {
            TokenKind::Let => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Break => {
                let s = self.span();
                self.advance();
                let label = if let TokenKind::Lifetime(name) = self.peek() {
                    let name = name.clone();
                    self.advance();
                    Some(name)
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon)?;
                Ok(Stmt::Break(label, s))
            }
            TokenKind::Continue => {
                let s = self.span();
                self.advance();
                let label = if let TokenKind::Lifetime(name) = self.peek() {
                    let name = name.clone();
                    self.advance();
                    Some(name)
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon)?;
                Ok(Stmt::Continue(label, s))
            }
            TokenKind::If => self.parse_if(),
            TokenKind::Lifetime(_) => {
                let label = if let TokenKind::Lifetime(name) = self.peek() {
                    name.clone()
                } else {
                    unreachable!()
                };
                let s = self.span();
                self.advance();
                self.expect(&TokenKind::Colon)?;
                match self.peek() {
                    TokenKind::Loop => {
                        self.advance();
                        let b = self.parse_block()?;
                        Ok(Stmt::Loop(Some(label), b, s))
                    }
                    TokenKind::While => self.parse_while_with_label(Some(label), s),
                    TokenKind::For => self.parse_for_with_label(Some(label), s),
                    _ => Err(self.error("expected loop, while, or for after label".to_string())),
                }
            }
            TokenKind::Loop => {
                let s = self.span();
                self.advance();
                let b = self.parse_block()?;
                Ok(Stmt::Loop(None, b, s))
            }
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Match => self.parse_match(),
            TokenKind::Static => self.parse_static_stmt(),
            _ => {
                let expr = self.parse_expr()?;
                // check for assignment
                if let Some(op) = self.try_assign_op() {
                    let value = self.parse_expr()?;
                    let span = Span::new(expr.span().start, value.span().end);
                    self.expect(&TokenKind::Semicolon)?;
                    Ok(Stmt::Assign {
                        target: expr,
                        op,
                        value,
                        span,
                    })
                } else {
                    self.expect(&TokenKind::Semicolon)?;
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn try_assign_op(&mut self) -> Option<AssignOp> {
        let op = match self.peek() {
            TokenKind::Assign => AssignOp::Assign,
            TokenKind::PlusEq => AssignOp::PlusEq,
            TokenKind::MinusEq => AssignOp::MinusEq,
            TokenKind::StarEq => AssignOp::StarEq,
            TokenKind::SlashEq => AssignOp::SlashEq,
            TokenKind::PercentEq => AssignOp::PercentEq,
            TokenKind::AmpEq => AssignOp::AmpEq,
            TokenKind::PipeEq => AssignOp::PipeEq,
            TokenKind::CaretEq => AssignOp::CaretEq,
            TokenKind::ShlEq => AssignOp::ShlEq,
            TokenKind::ShrEq => AssignOp::ShrEq,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    fn parse_let(&mut self) -> R<Stmt> {
        let start = self.span();
        self.advance(); // let
        let mutable = self.eat(&TokenKind::Mut);
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_expr()?;
        let end = self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Let {
            name,
            mutable,
            ty,
            value,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_return(&mut self) -> R<Stmt> {
        let start = self.span();
        self.advance(); // return
        if self.eat(&TokenKind::Semicolon) {
            return Ok(Stmt::Return(None, start));
        }
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon)?;
        Ok(Stmt::Return(Some(expr), start))
    }

    fn parse_if(&mut self) -> R<Stmt> {
        let start = self.span();
        self.advance(); // if
        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if self.eat(&TokenKind::Else) {
            if matches!(self.peek(), TokenKind::If) {
                Some(ElseBranch::ElseIf(Box::new(self.parse_if()?)))
            } else {
                Some(ElseBranch::Else(self.parse_block()?))
            }
        } else {
            None
        };
        Ok(Stmt::If {
            condition,
            then_block,
            else_block,
            span: start,
        })
    }

    fn parse_while(&mut self) -> R<Stmt> {
        self.parse_while_with_label(None, self.span())
    }

    fn parse_while_with_label(&mut self, label: Option<String>, start: Span) -> R<Stmt> {
        self.advance(); // while
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While {
            label,
            condition,
            body,
            span: start,
        })
    }

    fn parse_for(&mut self) -> R<Stmt> {
        self.parse_for_with_label(None, self.span())
    }

    fn parse_for_with_label(&mut self, label: Option<String>, start: Span) -> R<Stmt> {
        self.advance(); // for
        let var = self.expect_ident()?;
        self.expect(&TokenKind::In)?;
        let range_start = self.parse_expr()?;
        self.expect(&TokenKind::DotDot)?;
        let range_end = self.parse_expr()?;

        // optional #[bound(N)]
        let bound = if matches!(self.peek(), TokenKind::Hash) {
            self.advance();
            self.expect(&TokenKind::LBracket)?;
            self.expect_ident()?; // "bound"
            self.expect(&TokenKind::LParen)?;
            let n = self.expect_int()?;
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::RBracket)?;
            Some(n)
        } else {
            None
        };

        let body = self.parse_block()?;
        Ok(Stmt::For {
            label,
            var,
            start: range_start,
            end: range_end,
            bound,
            body,
            span: start,
        })
    }

    fn parse_match(&mut self) -> R<Stmt> {
        let start = self.span();
        self.advance(); // match
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            arms.push(self.parse_match_arm()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Stmt::Match {
            expr,
            arms,
            span: start,
        })
    }

    fn parse_match_arm(&mut self) -> R<MatchArm> {
        let start = self.span();
        let pattern = self.parse_pattern()?;
        self.expect(&TokenKind::FatArrow)?;
        let body = self.parse_expr()?;
        self.eat(&TokenKind::Comma);
        Ok(MatchArm {
            pattern,
            body,
            span: start,
        })
    }

    fn parse_pattern(&mut self) -> R<Pattern> {
        match self.peek().clone() {
            TokenKind::Underscore => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            TokenKind::IntLit(v) => {
                self.advance();
                Ok(Pattern::IntLit(v))
            }
            TokenKind::Ident(name) => {
                self.advance();
                if self.eat(&TokenKind::LParen) {
                    let mut bindings = Vec::new();
                    if !matches!(self.peek(), TokenKind::RParen) {
                        bindings.push(self.expect_ident()?);
                        while self.eat(&TokenKind::Comma) {
                            bindings.push(self.expect_ident()?);
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Pattern::Variant(name, bindings))
                } else {
                    Ok(Pattern::Ident(name))
                }
            }
            _ => Err(self.error(format!("expected pattern, got {:?}", self.peek()))),
        }
    }

    // static inside a function body (for interrupt handlers)
    fn parse_static_stmt(&mut self) -> R<Stmt> {
        let s = self.parse_static()?;
        Ok(Stmt::Let {
            name: s.name,
            mutable: s.mutable,
            ty: Some(s.ty),
            value: s.value,
            span: s.span,
        })
    }

    // ── expressions (Pratt parser) ──

    pub fn parse_expr(&mut self) -> R<Expr> {
        self.pratt(0)
    }

    // precedence table — higher number binds tighter
    fn prefix_bp(&self, op: &TokenKind) -> Option<((), u8)> {
        match op {
            TokenKind::Minus => Some(((), 25)),
            TokenKind::Bang => Some(((), 25)),
            TokenKind::Tilde => Some(((), 25)),
            TokenKind::Amp => Some(((), 25)),
            TokenKind::Star => Some(((), 25)),
            TokenKind::Try => Some(((), 25)),
            _ => None,
        }
    }

    fn infix_bp(&self, op: &TokenKind) -> Option<(u8, u8)> {
        // (left_bp, right_bp) — left < right = left-assoc
        match op {
            TokenKind::PipePipe => Some((2, 3)),
            TokenKind::AmpAmp => Some((4, 5)),
            TokenKind::Eq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge => Some((6, 7)),
            TokenKind::Pipe => Some((8, 9)),
            TokenKind::Caret => Some((10, 11)),
            TokenKind::Amp => Some((12, 13)),
            TokenKind::Shl | TokenKind::Shr => Some((14, 15)),
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::WrapPlus
            | TokenKind::WrapMinus
            | TokenKind::CheckPlus
            | TokenKind::CheckMinus => Some((16, 17)),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent | TokenKind::WrapStar => {
                Some((18, 19))
            }
            TokenKind::As => Some((20, 21)),
            _ => None,
        }
    }

    fn token_to_binop(&self, kind: &TokenKind) -> BinOp {
        match kind {
            TokenKind::Plus => BinOp::Add,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            TokenKind::Percent => BinOp::Rem,
            TokenKind::Amp => BinOp::BitAnd,
            TokenKind::Pipe => BinOp::BitOr,
            TokenKind::Caret => BinOp::BitXor,
            TokenKind::Shl => BinOp::Shl,
            TokenKind::Shr => BinOp::Shr,
            TokenKind::Eq => BinOp::Eq,
            TokenKind::Ne => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::Le => BinOp::Le,
            TokenKind::Ge => BinOp::Ge,
            TokenKind::AmpAmp => BinOp::And,
            TokenKind::PipePipe => BinOp::Or,
            TokenKind::WrapPlus => BinOp::WrapAdd,
            TokenKind::WrapMinus => BinOp::WrapSub,
            TokenKind::WrapStar => BinOp::WrapMul,
            TokenKind::CheckPlus => BinOp::CheckAdd,
            TokenKind::CheckMinus => BinOp::CheckSub,
            _ => unreachable!(),
        }
    }

    fn pratt(&mut self, min_bp: u8) -> R<Expr> {
        // prefix
        let mut lhs = if let Some(((), rbp)) = self.prefix_bp(self.peek()) {
            let start = self.span();
            let op_kind = self.peek().clone();
            self.advance();

            // &mut special case
            let unary_op = if matches!(op_kind, TokenKind::Amp) && self.eat(&TokenKind::Mut) {
                UnaryOp::RefMut
            } else {
                match op_kind {
                    TokenKind::Minus => UnaryOp::Neg,
                    TokenKind::Bang => UnaryOp::Not,
                    TokenKind::Tilde => UnaryOp::BitNot,
                    TokenKind::Amp => UnaryOp::Ref,
                    TokenKind::Star => UnaryOp::Deref,
                    TokenKind::Try => {
                        let inner = self.pratt(rbp)?;
                        let span = Span::new(start.start, inner.span().end);
                        return Ok(Expr::Try(Box::new(inner), span));
                    }
                    _ => unreachable!(),
                }
            };

            let operand = self.pratt(rbp)?;
            let span = Span::new(start.start, operand.span().end);
            Expr::Unary(unary_op, Box::new(operand), span)
        } else {
            self.parse_primary()?
        };

        // postfix: field access, method call, function call, index
        loop {
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    let field = self.expect_ident()?;
                    if self.eat(&TokenKind::LParen) {
                        let args = self.parse_arg_list()?;
                        self.expect(&TokenKind::RParen)?;
                        let span = Span::new(lhs.span().start, self.span().start);
                        lhs = Expr::MethodCall(Box::new(lhs), field, args, span);
                    } else {
                        let span = Span::new(lhs.span().start, self.span().start);
                        lhs = Expr::Field(Box::new(lhs), field, span);
                    }
                }
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&TokenKind::RParen)?;
                    let span = Span::new(lhs.span().start, self.span().start);
                    lhs = Expr::Call(Box::new(lhs), args, span);
                }
                TokenKind::LBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    let span = Span::new(lhs.span().start, self.span().start);
                    lhs = Expr::Index(Box::new(lhs), Box::new(idx), span);
                }
                _ => break,
            }
        }

        // infix
        loop {
            let op_kind = self.peek().clone();

            // 'as' cast
            if matches!(op_kind, TokenKind::As) {
                if let Some((lbp, _)) = self.infix_bp(&op_kind) {
                    if lbp < min_bp {
                        break;
                    }
                    self.advance();
                    let ty = self.parse_type()?;
                    let span = Span::new(lhs.span().start, self.span().start);
                    lhs = Expr::Cast(Box::new(lhs), ty, span);
                    continue;
                }
            }

            if let Some((lbp, rbp)) = self.infix_bp(&op_kind) {
                if lbp < min_bp {
                    break;
                }
                self.advance();
                let rhs = self.pratt(rbp)?;
                let span = Span::new(lhs.span().start, rhs.span().end);
                let op = self.token_to_binop(&op_kind);
                lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs), span);
            } else {
                break;
            }
        }

        Ok(lhs)
    }

    fn parse_primary(&mut self) -> R<Expr> {
        let start = self.span();
        match self.peek().clone() {
            TokenKind::IntLit(v) => {
                self.advance();
                Ok(Expr::IntLit(v, start))
            }
            TokenKind::FloatLit(v) => {
                self.advance();
                Ok(Expr::FloatLit(v, start))
            }
            TokenKind::StringLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::StringLit(s, start))
            }
            TokenKind::CharLit(c) => {
                self.advance();
                Ok(Expr::CharLit(c, start))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLit(true, start))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLit(false, start))
            }

            // .output shorthand (dot-enum)
            TokenKind::Dot => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Expr::DotEnum(
                    name,
                    Span::new(start.start, self.span().start),
                ))
            }

            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.advance();

                // struct literal: Name { field: value, ... }
                if matches!(self.peek(), TokenKind::LBrace) {
                    // only parse as struct lit if next token after { is ident followed by :
                    // this disambiguates from block expressions
                    if self.pos + 2 < self.tokens.len()
                        && matches!(self.tokens[self.pos + 1].kind, TokenKind::Ident(_))
                        && matches!(self.tokens[self.pos + 2].kind, TokenKind::Colon)
                    {
                        return self.parse_struct_lit(name, start);
                    }
                }

                Ok(Expr::Ident(name, start))
            }

            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }

            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                if !matches!(self.peek(), TokenKind::RBracket) {
                    elems.push(self.parse_expr()?);
                    while self.eat(&TokenKind::Comma) {
                        if matches!(self.peek(), TokenKind::RBracket) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::ArrayLit(
                    elems,
                    Span::new(start.start, self.span().start),
                ))
            }

            _ => Err(self.error(format!("expected expression, got {:?}", self.peek()))),
        }
    }

    fn parse_struct_lit(&mut self, name: String, start: Span) -> R<Expr> {
        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let value = self.parse_expr()?;
            fields.push((fname, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Expr::StructLit(
            name,
            fields,
            Span::new(start.start, self.span().start),
        ))
    }

    fn parse_arg_list(&mut self) -> R<Vec<Expr>> {
        let mut args = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while self.eat(&TokenKind::Comma) {
            if matches!(self.peek(), TokenKind::RParen) {
                break;
            }
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::tokenize(src).unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    #[test]
    fn parse_import() {
        let prog = parse("import board::esp32c3;");
        assert_eq!(prog.items.len(), 1);
        match &prog.items[0] {
            TopItem::Import(i) => assert_eq!(i.path, vec!["board", "esp32c3"]),
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn parse_board() {
        let prog = parse("board esp32c3 { gpio: GPIO @ 0x6000_4000, clock: 160_000_000, }");
        match &prog.items[0] {
            TopItem::Board(b) => {
                assert_eq!(b.name, "esp32c3");
                assert_eq!(b.fields.len(), 2);
                assert_eq!(b.fields[0].name, "gpio");
                assert_eq!(b.fields[1].name, "clock");
            }
            _ => panic!("expected board"),
        }
    }

    #[test]
    fn parse_function() {
        let prog = parse("fn main(b: &mut esp32c3) { let x: u32 = 42; }");
        match &prog.items[0] {
            TopItem::Function(f) => {
                assert_eq!(f.name, "main");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "b");
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_interrupt() {
        let prog = parse("interrupt(timer0, priority = 2) fn on_tick() { }");
        match &prog.items[0] {
            TopItem::Interrupt(i) => {
                assert_eq!(i.interrupt_name, "timer0");
                assert_eq!(i.priority, Some(2));
                assert_eq!(i.fn_name, "on_tick");
            }
            _ => panic!("expected interrupt"),
        }
    }

    #[test]
    fn parse_struct_and_enum() {
        let prog = parse("struct Point { x: u32, y: u32, } enum Color { Red, Green, Blue, }");
        assert_eq!(prog.items.len(), 2);
        match &prog.items[0] {
            TopItem::Struct(s) => {
                assert_eq!(s.name, "Point");
                assert_eq!(s.fields.len(), 2);
            }
            _ => panic!("expected struct"),
        }
        match &prog.items[1] {
            TopItem::Enum(e) => {
                assert_eq!(e.name, "Color");
                assert_eq!(e.variants.len(), 3);
            }
            _ => panic!("expected enum"),
        }
    }

    #[test]
    fn parse_expressions() {
        let prog = parse("fn f() { let x = 1 + 2 * 3; }");
        match &prog.items[0] {
            TopItem::Function(f) => {
                match &f.body.stmts[0] {
                    Stmt::Let { value, .. } => {
                        // should be Add(1, Mul(2, 3)) due to precedence
                        match value {
                            Expr::Binary(_, BinOp::Add, _, _) => {}
                            other => panic!("expected Add, got {:?}", other),
                        }
                    }
                    _ => panic!("expected let"),
                }
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_method_chain() {
        let prog = parse("fn f() { b.gpio.pin(2, .output).high(); }");
        assert_eq!(prog.items.len(), 1);
    }

    #[test]
    fn parse_for_with_bound() {
        let prog = parse("fn f() { for i in 0..n #[bound(100)] { x = i; } }");
        match &prog.items[0] {
            TopItem::Function(f) => match &f.body.stmts[0] {
                Stmt::For { var, bound, .. } => {
                    assert_eq!(var, "i");
                    assert_eq!(*bound, Some(100));
                }
                _ => panic!("expected for"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_attribute() {
        let prog = parse("#[max_cycles(200)] fn critical() { }");
        match &prog.items[0] {
            TopItem::Function(f) => {
                assert_eq!(f.attrs.len(), 1);
                assert_eq!(f.attrs[0].name, "max_cycles");
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_full_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        let tokens = Lexer::tokenize(&source).unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        // import + board + fn + interrupt = 4 items
        assert_eq!(prog.items.len(), 4);
    }

    #[test]
    fn error_recovery_multiple_errors() {
        let src = "fn f( { } fn g() { let x = ; }";
        let tokens = Lexer::tokenize(src).unwrap();
        let result = Parser::new(tokens).parse();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // should report at least 2 errors (one for f, one for g)
        assert!(
            errors.len() >= 2,
            "should recover and report multiple errors, got {}",
            errors.len()
        );
    }

    #[test]
    fn parse_extern_fn() {
        let prog = parse("extern \"C\" fn HAL_GPIO_Write(port: u32, pin: u32, state: u32);");
        assert_eq!(prog.items.len(), 1);
        match &prog.items[0] {
            TopItem::ExternFn(e) => {
                assert_eq!(e.abi, "C");
                assert_eq!(e.name, "HAL_GPIO_Write");
                assert_eq!(e.params.len(), 3);
            }
            _ => panic!("expected extern fn"),
        }
    }

    #[test]
    fn parse_extern_with_return() {
        let prog = parse("extern \"C\" fn read_sensor() u32;");
        match &prog.items[0] {
            TopItem::ExternFn(e) => {
                assert_eq!(e.name, "read_sensor");
                assert!(e.ret_type.is_some());
            }
            _ => panic!("expected extern fn"),
        }
    }

    #[test]
    fn parse_generic_fn() {
        let prog = parse("fn max<T: Ord>(a: T, b: T) T { return a; }");
        match &prog.items[0] {
            TopItem::Function(f) => {
                assert_eq!(f.name, "max");
                assert_eq!(f.type_params.len(), 1);
                assert_eq!(f.type_params[0].name, "T");
                assert_eq!(f.type_params[0].bounds, vec!["Ord"]);
                assert_eq!(f.params.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_generic_struct() {
        let prog = parse("struct Pair<A, B> { first: A, second: B }");
        match &prog.items[0] {
            TopItem::Struct(s) => {
                assert_eq!(s.name, "Pair");
                assert_eq!(s.type_params.len(), 2);
                assert_eq!(s.type_params[0].name, "A");
                assert_eq!(s.type_params[1].name, "B");
            }
            _ => panic!("expected struct"),
        }
    }
}
