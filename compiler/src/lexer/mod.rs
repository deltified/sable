use crate::diagnostics::Diagnostics;
use crate::source::{FileId, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Eof,
    Identifier,
    IntLiteral,
    FloatLiteral,
    StringLiteral,

    At,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,
    Dot,
    DotDot,
    Arrow,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    AndAnd,
    Amp,
    OrOr,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PlusPlus,

    KwFn,
    KwLet,
    KwReturn,
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwIn,
    KwStruct,
    KwImport,
    KwExtern,
    KwEffects,
    KwRef,
    KwTrue,
    KwFalse,
    KwBreak,
    KwContinue,
    KwTry,
    KwCatch,
    KwRaise,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub span: Span,
}

pub fn lex(file_id: FileId, source: &str) -> (Vec<Token>, Diagnostics) {
    let mut lexer = Lexer {
        file_id,
        source,
        bytes: source.as_bytes(),
        pos: 0,
        diagnostics: Diagnostics::new(),
    };

    let mut tokens = Vec::with_capacity(source.len() / 3 + 1);
    while let Some(byte) = lexer.peek() {
        if is_whitespace(byte) {
            lexer.bump();
            continue;
        }

        if byte == b'/' && lexer.peek_next() == Some(b'/') {
            lexer.bump();
            lexer.bump();
            while let Some(ch) = lexer.peek() {
                lexer.bump();
                if ch == b'\n' {
                    break;
                }
            }
            continue;
        }

        let start = lexer.pos;
        match byte {
            b'(' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::LParen, start));
            }
            b')' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::RParen, start));
            }
            b'{' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::LBrace, start));
            }
            b'}' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::RBrace, start));
            }
            b'[' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::LBracket, start));
            }
            b']' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::RBracket, start));
            }
            b',' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::Comma, start));
            }
            b':' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::Colon, start));
            }
            b';' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::Semicolon, start));
            }
            b'@' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::At, start));
            }
            b'.' => {
                lexer.bump();
                if lexer.peek() == Some(b'.') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::DotDot, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Dot, start));
                }
            }
            b'+' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::PlusEq, start));
                } else if lexer.peek() == Some(b'+') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::PlusPlus, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Plus, start));
                }
            }
            b'-' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::MinusEq, start));
                } else if lexer.peek() == Some(b'>') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::Arrow, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Minus, start));
                }
            }
            b'*' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::StarEq, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Star, start));
                }
            }
            b'/' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::SlashEq, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Slash, start));
                }
            }
            b'%' => {
                lexer.bump();
                tokens.push(lexer.token(TokenKind::Percent, start));
            }
            b'=' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::EqEq, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Eq, start));
                }
            }
            b'!' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::NotEq, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Bang, start));
                }
            }
            b'<' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::Lte, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Lt, start));
                }
            }
            b'>' => {
                lexer.bump();
                if lexer.peek() == Some(b'=') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::Gte, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Gt, start));
                }
            }
            b'&' => {
                lexer.bump();
                if lexer.peek() == Some(b'&') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::AndAnd, start));
                } else {
                    tokens.push(lexer.token(TokenKind::Amp, start));
                }
            }
            b'|' => {
                lexer.bump();
                if lexer.peek() == Some(b'|') {
                    lexer.bump();
                    tokens.push(lexer.token(TokenKind::OrOr, start));
                } else {
                    lexer.diagnostics.error(
                        "LEX002",
                        "unexpected '|'; expected '||'",
                        Some(Span::new(lexer.file_id, start, lexer.pos)),
                    );
                }
            }
            b'"' => tokens.push(lexer.lex_string()),
            b'0'..=b'9' => tokens.push(lexer.lex_number()),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => tokens.push(lexer.lex_identifier()),
            _ => {
                let mut end = lexer.pos + 1;
                while !source.is_char_boundary(end) && end < source.len() {
                    end += 1;
                }
                lexer.pos = end;
                lexer.diagnostics.error(
                    "LEX003",
                    "invalid character",
                    Some(Span::new(lexer.file_id, start, end)),
                );
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        text: String::new(),
        span: Span::new(file_id, source.len(), source.len()),
    });

    (tokens, lexer.diagnostics)
}

struct Lexer<'a> {
    file_id: FileId,
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    diagnostics: Diagnostics,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn token(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            text: self.source[start..self.pos].to_string(),
            span: Span::new(self.file_id, start, self.pos),
        }
    }

    fn lex_identifier(&mut self) -> Token {
        let start = self.pos;
        self.bump();
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                self.bump();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.pos];
        let kind = match text {
            "fn" => TokenKind::KwFn,
            "let" => TokenKind::KwLet,
            "return" => TokenKind::KwReturn,
            "if" => TokenKind::KwIf,
            "else" => TokenKind::KwElse,
            "while" => TokenKind::KwWhile,
            "for" => TokenKind::KwFor,
            "in" => TokenKind::KwIn,
            "struct" => TokenKind::KwStruct,
            "import" => TokenKind::KwImport,
            "extern" => TokenKind::KwExtern,
            "effects" => TokenKind::KwEffects,
            "ref" => TokenKind::KwRef,
            "true" => TokenKind::KwTrue,
            "false" => TokenKind::KwFalse,
            "break" => TokenKind::KwBreak,
            "continue" => TokenKind::KwContinue,
            "try" => TokenKind::KwTry,
            "catch" => TokenKind::KwCatch,
            "raise" => TokenKind::KwRaise,
            _ => TokenKind::Identifier,
        };

        Token {
            kind,
            text: text.to_string(),
            span: Span::new(self.file_id, start, self.pos),
        }
    }

    fn lex_number(&mut self) -> Token {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'_')) {
            self.bump();
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') && self.peek_next().is_some_and(|n| n.is_ascii_digit()) {
            is_float = true;
            self.bump();
            while matches!(self.peek(), Some(b'0'..=b'9' | b'_')) {
                self.bump();
            }
        }

        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            while matches!(self.peek(), Some(b'0'..=b'9' | b'_')) {
                self.bump();
            }
        }

        while matches!(self.peek(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')) {
            self.bump();
        }

        let text = self.source[start..self.pos].to_string();
        let kind = if is_float {
            TokenKind::FloatLiteral
        } else {
            TokenKind::IntLiteral
        };

        Token {
            kind,
            text,
            span: Span::new(self.file_id, start, self.pos),
        }
    }

    fn lex_string(&mut self) -> Token {
        let start = self.pos;
        self.bump();
        let mut terminated = false;
        while let Some(ch) = self.peek() {
            self.bump();
            if ch == b'\\' {
                if self.peek().is_some() {
                    self.bump();
                }
                continue;
            }
            if ch == b'"' {
                terminated = true;
                break;
            }
            if ch == b'\n' {
                break;
            }
        }

        if !terminated {
            self.diagnostics.error(
                "LEX004",
                "unterminated string literal",
                Some(Span::new(self.file_id, start, self.pos)),
            );
        }

        Token {
            kind: TokenKind::StringLiteral,
            text: self.source[start..self.pos].to_string(),
            span: Span::new(self.file_id, start, self.pos),
        }
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn is_ident_continue(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}
