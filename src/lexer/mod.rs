pub mod token;

use token::{Span, Token, TokenKind};

/// Hand-written lexer. Single pass over the source bytes.
/// No regex, no external dependencies.
///
/// Design choices:
/// - Operates on &[u8] not &str — avoids UTF-8 boundary checks in the hot loop
/// - Identifier/keyword lookup via TokenKind::keyword() — one branch
/// - Numbers support _ separators and 0x/0b/0o prefixes
/// - Strings support standard escape sequences
/// - Line comments (//) and block comments (/* */ nestable)
pub struct Lexer<'src> {
    src: &'src [u8],
    pos: u32,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            src: source.as_bytes(),
            pos: 0,
        }
    }

    /// Tokenize the entire source into a Vec<Token>.
    /// Includes a final Eof token.
    pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> u8 {
        if self.pos as usize >= self.src.len() {
            0
        } else {
            self.src[self.pos as usize]
        }
    }

    fn peek_at(&self, offset: u32) -> u8 {
        let idx = (self.pos + offset) as usize;
        if idx >= self.src.len() {
            0
        } else {
            self.src[idx]
        }
    }

    fn advance(&mut self) -> u8 {
        let b = self.peek();
        self.pos += 1;
        b
    }

    fn at_end(&self) -> bool {
        self.pos as usize >= self.src.len()
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // whitespace
            while !self.at_end() && self.peek().is_ascii_whitespace() {
                self.advance();
            }

            // line comment
            if self.peek() == b'/' && self.peek_at(1) == b'/' {
                while !self.at_end() && self.peek() != b'\n' {
                    self.advance();
                }
                continue;
            }

            // block comment (nestable)
            if self.peek() == b'/' && self.peek_at(1) == b'*' {
                self.advance();
                self.advance();
                let mut depth = 1u32;
                while !self.at_end() && depth > 0 {
                    if self.peek() == b'/' && self.peek_at(1) == b'*' {
                        self.advance();
                        self.advance();
                        depth += 1;
                    } else if self.peek() == b'*' && self.peek_at(1) == b'/' {
                        self.advance();
                        self.advance();
                        depth -= 1;
                    } else {
                        self.advance();
                    }
                }
                continue;
            }

            break;
        }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();

        if self.at_end() {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(self.pos, self.pos),
            });
        }

        let start = self.pos;
        let b = self.peek();

        // identifier or keyword
        if b.is_ascii_alphabetic() || b == b'_' {
            return Ok(self.lex_ident(start));
        }

        // number literal
        if b.is_ascii_digit() {
            return self.lex_number(start);
        }

        // string literal
        if b == b'"' {
            return self.lex_string(start);
        }

        // char literal
        if b == b'\'' {
            return self.lex_char(start);
        }

        // punctuation and operators
        self.lex_punct(start)
    }

    fn lex_ident(&mut self, start: u32) -> Token {
        while !self.at_end() && (self.peek().is_ascii_alphanumeric() || self.peek() == b'_') {
            self.advance();
        }
        let text = std::str::from_utf8(&self.src[start as usize..self.pos as usize]).unwrap();
        let kind = TokenKind::keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token {
            kind,
            span: Span::new(start, self.pos),
        }
    }

    fn lex_number(&mut self, start: u32) -> Result<Token, LexError> {
        // check prefix
        if self.peek() == b'0' && self.pos + 1 < self.src.len() as u32 {
            match self.peek_at(1) {
                b'x' | b'X' => return self.lex_hex(start),
                b'b' | b'B' => return self.lex_bin(start),
                b'o' | b'O' => return self.lex_oct(start),
                _ => {}
            }
        }

        // decimal
        while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
            self.advance();
        }

        // check for float
        if self.peek() == b'.' && self.peek_at(1).is_ascii_digit() {
            self.advance(); // skip '.'
            while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
                self.advance();
            }
            let text = std::str::from_utf8(&self.src[start as usize..self.pos as usize]).unwrap();
            let clean: String = text.chars().filter(|c| *c != '_').collect();
            let val: f64 = clean
                .parse()
                .map_err(|_| LexError::InvalidNumber(start))?;
            return Ok(Token {
                kind: TokenKind::FloatLit(val),
                span: Span::new(start, self.pos),
            });
        }

        let text = std::str::from_utf8(&self.src[start as usize..self.pos as usize]).unwrap();
        let clean: String = text.chars().filter(|c| *c != '_').collect();
        let val: u64 = clean
            .parse()
            .map_err(|_| LexError::InvalidNumber(start))?;
        Ok(Token {
            kind: TokenKind::IntLit(val),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_hex(&mut self, start: u32) -> Result<Token, LexError> {
        self.advance(); // '0'
        self.advance(); // 'x'
        let digits_start = self.pos;
        while !self.at_end() && (self.peek().is_ascii_hexdigit() || self.peek() == b'_') {
            self.advance();
        }
        if self.pos == digits_start {
            return Err(LexError::InvalidNumber(start));
        }
        let text =
            std::str::from_utf8(&self.src[digits_start as usize..self.pos as usize]).unwrap();
        let clean: String = text.chars().filter(|c| *c != '_').collect();
        let val = u64::from_str_radix(&clean, 16).map_err(|_| LexError::InvalidNumber(start))?;
        Ok(Token {
            kind: TokenKind::IntLit(val),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_bin(&mut self, start: u32) -> Result<Token, LexError> {
        self.advance(); // '0'
        self.advance(); // 'b'
        let digits_start = self.pos;
        while !self.at_end() && (self.peek() == b'0' || self.peek() == b'1' || self.peek() == b'_')
        {
            self.advance();
        }
        if self.pos == digits_start {
            return Err(LexError::InvalidNumber(start));
        }
        let text =
            std::str::from_utf8(&self.src[digits_start as usize..self.pos as usize]).unwrap();
        let clean: String = text.chars().filter(|c| *c != '_').collect();
        let val = u64::from_str_radix(&clean, 2).map_err(|_| LexError::InvalidNumber(start))?;
        Ok(Token {
            kind: TokenKind::IntLit(val),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_oct(&mut self, start: u32) -> Result<Token, LexError> {
        self.advance(); // '0'
        self.advance(); // 'o'
        let digits_start = self.pos;
        while !self.at_end()
            && ((self.peek() >= b'0' && self.peek() <= b'7') || self.peek() == b'_')
        {
            self.advance();
        }
        if self.pos == digits_start {
            return Err(LexError::InvalidNumber(start));
        }
        let text =
            std::str::from_utf8(&self.src[digits_start as usize..self.pos as usize]).unwrap();
        let clean: String = text.chars().filter(|c| *c != '_').collect();
        let val = u64::from_str_radix(&clean, 8).map_err(|_| LexError::InvalidNumber(start))?;
        Ok(Token {
            kind: TokenKind::IntLit(val),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_string(&mut self, start: u32) -> Result<Token, LexError> {
        self.advance(); // opening "
        let mut s = String::new();
        while !self.at_end() && self.peek() != b'"' {
            if self.peek() == b'\\' {
                self.advance();
                match self.advance() {
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'\\' => s.push('\\'),
                    b'\'' => s.push('\''),
                    b'"' => s.push('"'),
                    b'0' => s.push('\0'),
                    b'x' => {
                        let hi = self.advance();
                        let lo = self.advance();
                        let val = hex_digit(hi)? * 16 + hex_digit(lo)?;
                        s.push(val as char);
                    }
                    _ => return Err(LexError::InvalidEscape(self.pos - 1)),
                }
            } else {
                s.push(self.advance() as char);
            }
        }
        if self.at_end() {
            return Err(LexError::UnterminatedString(start));
        }
        self.advance(); // closing "
        Ok(Token {
            kind: TokenKind::StringLit(s),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_char(&mut self, start: u32) -> Result<Token, LexError> {
        self.advance(); // opening '
        let ch = if self.peek() == b'\\' {
            self.advance();
            match self.advance() {
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'\\' => '\\',
                b'\'' => '\'',
                b'0' => '\0',
                _ => return Err(LexError::InvalidEscape(self.pos - 1)),
            }
        } else {
            self.advance() as char
        };
        if self.peek() != b'\'' {
            return Err(LexError::UnterminatedChar(start));
        }
        self.advance(); // closing '
        Ok(Token {
            kind: TokenKind::CharLit(ch),
            span: Span::new(start, self.pos),
        })
    }

    fn lex_punct(&mut self, start: u32) -> Result<Token, LexError> {
        let b = self.advance();
        let kind = match b {
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'~' => TokenKind::Tilde,
            b'@' => TokenKind::At,
            b'#' => TokenKind::Hash,
            b'_' => TokenKind::Underscore,

            b':' => {
                if self.peek() == b':' {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            b'.' => {
                if self.peek() == b'.' {
                    self.advance();
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                }
            }
            b'-' => {
                if self.peek() == b'>' {
                    self.advance();
                    TokenKind::Arrow
                } else if self.peek() == b'=' {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            b'=' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Eq
                } else if self.peek() == b'>' {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Assign
                }
            }
            b'+' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            b'*' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            b'/' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            b'%' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::PercentEq
                } else if self.peek() == b'+' {
                    self.advance();
                    TokenKind::WrapPlus
                } else if self.peek() == b'-' {
                    self.advance();
                    TokenKind::WrapMinus
                } else if self.peek() == b'*' {
                    self.advance();
                    TokenKind::WrapStar
                } else {
                    TokenKind::Percent
                }
            }
            b'!' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Ne
                } else {
                    TokenKind::Bang
                }
            }
            b'<' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Le
                } else if self.peek() == b'<' {
                    self.advance();
                    if self.peek() == b'=' {
                        self.advance();
                        TokenKind::ShlEq
                    } else {
                        TokenKind::Shl
                    }
                } else {
                    TokenKind::Lt
                }
            }
            b'>' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::Ge
                } else if self.peek() == b'>' {
                    self.advance();
                    if self.peek() == b'=' {
                        self.advance();
                        TokenKind::ShrEq
                    } else {
                        TokenKind::Shr
                    }
                } else {
                    TokenKind::Gt
                }
            }
            b'&' => {
                if self.peek() == b'&' {
                    self.advance();
                    TokenKind::AmpAmp
                } else if self.peek() == b'=' {
                    self.advance();
                    TokenKind::AmpEq
                } else {
                    TokenKind::Amp
                }
            }
            b'|' => {
                if self.peek() == b'|' {
                    self.advance();
                    TokenKind::PipePipe
                } else if self.peek() == b'=' {
                    self.advance();
                    TokenKind::PipeEq
                } else {
                    TokenKind::Pipe
                }
            }
            b'^' => {
                if self.peek() == b'=' {
                    self.advance();
                    TokenKind::CaretEq
                } else {
                    TokenKind::Caret
                }
            }
            b'?' => {
                if self.peek() == b'+' {
                    self.advance();
                    TokenKind::CheckPlus
                } else if self.peek() == b'-' {
                    self.advance();
                    TokenKind::CheckMinus
                } else {
                    return Err(LexError::UnexpectedChar(start, b as char));
                }
            }
            _ => return Err(LexError::UnexpectedChar(start, b as char)),
        };

        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
        })
    }
}

fn hex_digit(b: u8) -> Result<u8, LexError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(LexError::InvalidEscape(0)),
    }
}

#[derive(Debug)]
pub enum LexError {
    UnexpectedChar(u32, char),
    InvalidNumber(u32),
    InvalidEscape(u32),
    UnterminatedString(u32),
    UnterminatedChar(u32),
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexError::UnexpectedChar(pos, ch) => write!(f, "unexpected character '{ch}' at {pos}"),
            LexError::InvalidNumber(pos) => write!(f, "invalid number at {pos}"),
            LexError::InvalidEscape(pos) => write!(f, "invalid escape sequence at {pos}"),
            LexError::UnterminatedString(pos) => write!(f, "unterminated string at {pos}"),
            LexError::UnterminatedChar(pos) => write!(f, "unterminated char at {pos}"),
        }
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_board_definition() {
        let tokens = Lexer::tokenize(
            r#"board esp32c3 {
                gpio: GPIO @ 0x6000_4000,
                clock: 160_000_000,
            }"#,
        )
        .unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::Board));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "esp32c3"));
        assert!(matches!(tokens[2].kind, TokenKind::LBrace));
        assert!(matches!(tokens[3].kind, TokenKind::Ident(ref s) if s == "gpio"));
        assert!(matches!(tokens[4].kind, TokenKind::Colon));
        assert!(matches!(tokens[5].kind, TokenKind::Ident(ref s) if s == "GPIO"));
        assert!(matches!(tokens[6].kind, TokenKind::At));
        assert!(matches!(tokens[7].kind, TokenKind::IntLit(0x60004000)));
        assert!(matches!(tokens[8].kind, TokenKind::Comma));
    }

    #[test]
    fn lex_function() {
        let tokens = Lexer::tokenize("fn main(b: &mut esp32c3) { let led = b.gpio.pin(2); }")
            .unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::Fn));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "main"));
        assert!(matches!(tokens[2].kind, TokenKind::LParen));
    }

    #[test]
    fn lex_numbers() {
        let tokens = Lexer::tokenize("42 0xFF 0b1010 0o77 1_000_000 3.14").unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::IntLit(42)));
        assert!(matches!(tokens[1].kind, TokenKind::IntLit(255)));
        assert!(matches!(tokens[2].kind, TokenKind::IntLit(10)));
        assert!(matches!(tokens[3].kind, TokenKind::IntLit(63)));
        assert!(matches!(tokens[4].kind, TokenKind::IntLit(1_000_000)));
        assert!(matches!(tokens[5].kind, TokenKind::FloatLit(f) if (f - 3.14).abs() < 0.001));
    }

    #[test]
    fn lex_strings_and_escapes() {
        let tokens = Lexer::tokenize(r#""hello\nworld" '\t' "0x\x41""#).unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::StringLit(ref s) if s == "hello\nworld"));
        assert!(matches!(tokens[1].kind, TokenKind::CharLit('\t')));
        assert!(matches!(tokens[2].kind, TokenKind::StringLit(ref s) if s == "0xA"));
    }

    #[test]
    fn lex_operators() {
        let tokens = Lexer::tokenize("== != <= >= << >> && || += -= => -> :: ..").unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::Eq));
        assert!(matches!(tokens[1].kind, TokenKind::Ne));
        assert!(matches!(tokens[2].kind, TokenKind::Le));
        assert!(matches!(tokens[3].kind, TokenKind::Ge));
        assert!(matches!(tokens[4].kind, TokenKind::Shl));
        assert!(matches!(tokens[5].kind, TokenKind::Shr));
        assert!(matches!(tokens[6].kind, TokenKind::AmpAmp));
        assert!(matches!(tokens[7].kind, TokenKind::PipePipe));
        assert!(matches!(tokens[8].kind, TokenKind::PlusEq));
        assert!(matches!(tokens[9].kind, TokenKind::MinusEq));
        assert!(matches!(tokens[10].kind, TokenKind::FatArrow));
        assert!(matches!(tokens[11].kind, TokenKind::Arrow));
        assert!(matches!(tokens[12].kind, TokenKind::ColonColon));
        assert!(matches!(tokens[13].kind, TokenKind::DotDot));
    }

    #[test]
    fn lex_overflow_operators() {
        let tokens = Lexer::tokenize("%+ %- %* ?+ ?-").unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::WrapPlus));
        assert!(matches!(tokens[1].kind, TokenKind::WrapMinus));
        assert!(matches!(tokens[2].kind, TokenKind::WrapStar));
        assert!(matches!(tokens[3].kind, TokenKind::CheckPlus));
        assert!(matches!(tokens[4].kind, TokenKind::CheckMinus));
    }

    #[test]
    fn lex_comments() {
        let tokens = Lexer::tokenize(
            "let x = 1; // this is a comment\nlet y = /* nested /* block */ comment */ 2;",
        )
        .unwrap();

        // should see: let x = 1 ; let y = 2 ; Eof
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(matches!(tokens[3].kind, TokenKind::IntLit(1)));
        assert!(matches!(tokens[5].kind, TokenKind::Let));
        assert!(matches!(tokens[8].kind, TokenKind::IntLit(2)));
    }

    #[test]
    fn lex_keywords_vs_idents() {
        let tokens = Lexer::tokenize("fn board let_it_go u32 my_u32").unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::Fn));
        assert!(matches!(tokens[1].kind, TokenKind::Board));
        assert!(matches!(tokens[2].kind, TokenKind::Ident(ref s) if s == "let_it_go"));
        assert!(matches!(tokens[3].kind, TokenKind::U32));
        assert!(matches!(tokens[4].kind, TokenKind::Ident(ref s) if s == "my_u32"));
    }

    #[test]
    fn lex_interrupt_annotation() {
        let tokens =
            Lexer::tokenize("interrupt(timer0, priority = 2) fn on_tick() {}").unwrap();

        assert!(matches!(tokens[0].kind, TokenKind::Interrupt));
        assert!(matches!(tokens[1].kind, TokenKind::LParen));
        assert!(matches!(tokens[2].kind, TokenKind::Ident(ref s) if s == "timer0"));
    }

    #[test]
    fn lex_full_program() {
        let source = r#"
            import board::esp32c3;

            board esp32c3 {
                gpio: GPIO @ 0x6000_4000,
                uart: UART @ 0x6000_0000,
                clock: 160_000_000,
            }

            fn main(b: &mut esp32c3) {
                let led = b.gpio.pin(2, .output);
                let tx = b.uart.open(115200);

                loop {
                    led.high();
                    delay_ms(500);
                    led.low();
                    delay_ms(500);
                    tx.write("blink\n");
                }
            }
        "#;

        let tokens = Lexer::tokenize(source).unwrap();
        // just verify it doesn't error and ends with Eof
        assert!(matches!(tokens.last().unwrap().kind, TokenKind::Eof));
        assert!(tokens.len() > 50); // substantial program
    }
}
