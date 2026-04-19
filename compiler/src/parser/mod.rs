use crate::ast::*;
use crate::diagnostics::Diagnostics;
use crate::lexer::{Token, TokenKind};
use crate::source::Span;

pub fn parse(tokens: Vec<Token>) -> (Module, Diagnostics) {
    let mut parser = Parser {
        tokens,
        idx: 0,
        diagnostics: Diagnostics::new(),
    };

    let module = parser.parse_module();
    (module, parser.diagnostics)
}

struct Parser {
    tokens: Vec<Token>,
    idx: usize,
    diagnostics: Diagnostics,
}

impl Parser {
    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();

        while !self.at(TokenKind::Eof) {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                self.bump();
            }
        }

        Module { items }
    }

    fn parse_item(&mut self) -> Option<Item> {
        if self.at(TokenKind::KwImport) {
            return self.parse_import().map(Item::Import);
        }

        let attrs = self.parse_attributes();

        if self.at(TokenKind::KwStruct) {
            return self.parse_struct(attrs).map(Item::Struct);
        }

        if self.at(TokenKind::KwExtern) || self.at(TokenKind::KwFn) {
            return self.parse_function(attrs).map(Item::Function);
        }

        if self.at(TokenKind::Eof) {
            return None;
        }

        self.diagnostics.error(
            "PAR001",
            format!(
                "unexpected token '{}' while parsing item",
                self.current().text
            ),
            Some(self.current().span),
        );
        None
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let start = self
            .expect(TokenKind::KwImport, "expected 'import' keyword")?
            .span;
        let mut path = Vec::new();

        let first = self.expect(TokenKind::Identifier, "expected module name after import")?;
        path.push(first.text.clone());

        while self.at(TokenKind::Dot) {
            self.bump();
            let part = self.expect(TokenKind::Identifier, "expected module path segment")?;
            path.push(part.text.clone());
        }

        let span = Span::new(start.file_id, start.start, self.prev_span().end);
        Some(ImportDecl { path, span })
    }

    fn parse_struct(&mut self, attrs: Vec<Attribute>) -> Option<StructDecl> {
        let start = self
            .expect(TokenKind::KwStruct, "expected 'struct' keyword")?
            .span;
        let name = self
            .expect(TokenKind::Identifier, "expected struct name")?
            .text;

        self.expect(TokenKind::LBrace, "expected '{' after struct name")?;
        let mut fields = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let field_name = self
                .expect(TokenKind::Identifier, "expected field name")?
                .text;
            self.expect(TokenKind::Colon, "expected ':' after field name")?;
            let field_ty = self.parse_type()?;
            let field_span = Span::new(start.file_id, self.prev_span().start, self.prev_span().end);
            fields.push(FieldDecl {
                name: field_name,
                ty: field_ty,
                span: field_span,
            });
            if self.at(TokenKind::Comma) {
                self.bump();
            }
        }

        let end = self
            .expect(TokenKind::RBrace, "expected '}' after struct fields")?
            .span;
        Some(StructDecl {
            attrs,
            name,
            fields,
            span: Span::new(start.file_id, start.start, end.end),
        })
    }

    fn parse_function(&mut self, attrs: Vec<Attribute>) -> Option<FunctionDecl> {
        let mut is_extern = false;
        let mut extern_abi = None;

        let start_span = if self.at(TokenKind::KwExtern) {
            is_extern = true;
            let extern_span = self.bump().span;
            if self.at(TokenKind::StringLiteral) {
                let abi_token = self.bump();
                extern_abi = Some(unquote(&abi_token.text));
            }
            extern_span
        } else {
            self.current().span
        };

        self.expect(TokenKind::KwFn, "expected 'fn' keyword")?;
        let name = self
            .expect(TokenKind::Identifier, "expected function name")?
            .text;

        self.expect(TokenKind::LParen, "expected '(' in function signature")?;
        let mut params = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let mut param_region = None;
            let mut param_is_ref = false;
            if self.at(TokenKind::KwRef) {
                param_is_ref = true;
                self.bump();
                if self.at(TokenKind::Lt) {
                    self.bump();
                    let region = self
                        .expect(TokenKind::Identifier, "expected region name after ref<")?
                        .text;
                    self.expect(TokenKind::Gt, "expected '>' after region name")?;
                    param_region = Some(region);
                }
            }

            let param_name = self
                .expect(TokenKind::Identifier, "expected parameter name")?
                .text;
            self.expect(TokenKind::Colon, "expected ':' after parameter name")?;
            let mut param_ty = self.parse_type()?;
            if param_is_ref {
                param_ty = TypeSyntax::Ref {
                    region: param_region,
                    mutable: true,
                    inner: Box::new(param_ty),
                };
            }
            let param_span = self.prev_span();
            params.push(ParamDecl {
                name: param_name,
                ty: param_ty,
                span: param_span,
            });

            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' in function signature")?;

        let return_type = if self.at(TokenKind::Arrow) {
            self.bump();
            self.parse_type()?
        } else {
            TypeSyntax::Void
        };

        let effects = if self.at(TokenKind::KwEffects) {
            self.parse_effects()?
        } else {
            EffectSyntax::all()
        };

        let trailing_attrs = self.parse_attributes();

        let body = if self.at(TokenKind::LBrace) {
            Some(self.parse_block()?)
        } else {
            None
        };

        if !is_extern && body.is_none() {
            self.diagnostics.error(
                "PAR002",
                format!("function '{name}' requires a body"),
                Some(self.current().span),
            );
        }

        let end = body
            .as_ref()
            .map(|b| b.span)
            .unwrap_or_else(|| self.prev_span());

        Some(FunctionDecl {
            attrs,
            trailing_attrs,
            name,
            params,
            return_type,
            effects,
            body,
            is_extern,
            extern_abi,
            span: Span::new(start_span.file_id, start_span.start, end.end),
        })
    }

    fn parse_effects(&mut self) -> Option<EffectSyntax> {
        self.expect(TokenKind::KwEffects, "expected effects clause")?;
        self.expect(TokenKind::LParen, "expected '(' after effects")?;

        let mut effect_syntax = EffectSyntax::none();

        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let token = self.expect(TokenKind::Identifier, "expected effect name")?;
            if token.text == "all" {
                effect_syntax.all = true;
            } else if token.text == "none" {
                effect_syntax.all = false;
            } else if token.text == "raise" {
                self.expect(TokenKind::LParen, "expected '(' after raise")?;
                while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                    let err_name = self
                        .expect(
                            TokenKind::Identifier,
                            "expected error type in raise(...) clause",
                        )?
                        .text;
                    effect_syntax.raises.push(err_name);
                    if self.at(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::RParen, "expected ')' after raise effect")?;
            } else {
                effect_syntax.effects.push(token.text);
            }

            if self.at(TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }

        self.expect(TokenKind::RParen, "expected ')' after effects list")?;
        Some(effect_syntax)
    }

    fn parse_attributes(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        while self.at(TokenKind::At) {
            let start = self.bump().span;
            let Some(name_token) = self.expect(TokenKind::Identifier, "expected attribute name")
            else {
                break;
            };

            let mut args = Vec::new();
            if self.at(TokenKind::LParen) {
                self.bump();
                while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                    let current = self.current().clone();
                    match current.kind {
                        TokenKind::Identifier => {
                            let key = self.bump().text;
                            if self.at(TokenKind::Eq) {
                                self.bump();
                                let value = self.bump();
                                args.push(AttrArg::KeyValue(key, value.text));
                            } else {
                                args.push(AttrArg::Ident(key));
                            }
                        }
                        TokenKind::StringLiteral => {
                            args.push(AttrArg::String(unquote(&self.bump().text)));
                        }
                        TokenKind::IntLiteral | TokenKind::FloatLiteral => {
                            args.push(AttrArg::Number(self.bump().text));
                        }
                        _ => {
                            self.diagnostics.error(
                                "PAR003",
                                "invalid attribute argument",
                                Some(self.current().span),
                            );
                            self.bump();
                        }
                    }

                    if self.at(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let _ = self.expect(TokenKind::RParen, "expected ')' after attribute arguments");
            }

            attrs.push(Attribute {
                name: name_token.text,
                args,
                span: Span::new(start.file_id, start.start, self.prev_span().end),
            });
        }

        attrs
    }

    fn parse_type(&mut self) -> Option<TypeSyntax> {
        if self.at(TokenKind::Amp) {
            self.bump();
            let inner = self.parse_type()?;
            return Some(TypeSyntax::Ref {
                region: None,
                mutable: false,
                inner: Box::new(inner),
            });
        }

        if self.at(TokenKind::KwRef) {
            self.bump();
            let region = if self.at(TokenKind::Lt) {
                self.bump();
                let region = self
                    .expect(TokenKind::Identifier, "expected region name after ref<")?
                    .text;
                self.expect(TokenKind::Gt, "expected '>' after region name")?;
                Some(region)
            } else {
                None
            };
            let inner = self.parse_type()?;
            return Some(TypeSyntax::Ref {
                region,
                mutable: true,
                inner: Box::new(inner),
            });
        }

        if self.at(TokenKind::LBracket) {
            self.bump();
            let inner = self.parse_type()?;
            let mut size = None;
            if self.at(TokenKind::Semicolon) {
                self.bump();
                let size_token = self.expect(TokenKind::IntLiteral, "expected array size")?;
                size = parse_usize_literal(&size_token.text);
            }
            self.expect(TokenKind::RBracket, "expected ']' after array type")?;
            return Some(TypeSyntax::Array {
                inner: Box::new(inner),
                size,
            });
        }

        if self.at(TokenKind::Identifier) {
            let name = self.bump().text;
            if self.at(TokenKind::Lt) {
                self.bump();

                let mut args = Vec::new();
                while !self.at(TokenKind::Gt) && !self.at(TokenKind::Eof) {
                    args.push(self.parse_type()?);
                    if self.at(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }

                self.expect(TokenKind::Gt, "expected '>' after generic type arguments")?;
                return Some(TypeSyntax::Generic { name, args });
            }

            return Some(TypeSyntax::Named(name));
        }

        self.diagnostics.error(
            "PAR004",
            format!("expected type, found '{}'", self.current().text),
            Some(self.current().span),
        );
        None
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self
            .expect(TokenKind::LBrace, "expected '{' to start block")?
            .span;
        let mut statements = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if let Some(stmt) = self.parse_stmt() {
                statements.push(stmt);
            } else {
                self.bump();
            }
        }
        let end = self
            .expect(TokenKind::RBrace, "expected '}' to close block")?
            .span;
        Some(Block {
            statements,
            span: Span::new(start.file_id, start.start, end.end),
        })
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        if self.at(TokenKind::KwLet) {
            return self.parse_let_stmt();
        }
        if self.at(TokenKind::KwReturn) {
            return self.parse_return_stmt();
        }
        if self.at(TokenKind::KwRaise) {
            return self.parse_raise_stmt();
        }
        if self.at(TokenKind::KwIf) {
            return self.parse_if_stmt();
        }
        if self.at(TokenKind::KwWhile) {
            return self.parse_while_stmt();
        }
        if self.at(TokenKind::KwFor) {
            return self.parse_for_stmt();
        }
        if self.at(TokenKind::KwBreak) {
            let span = self.bump().span;
            self.consume_optional_semicolon();
            return Some(Stmt::Break(span));
        }
        if self.at(TokenKind::KwContinue) {
            let span = self.bump().span;
            self.consume_optional_semicolon();
            return Some(Stmt::Continue(span));
        }
        if self.at(TokenKind::LBrace) {
            return Some(Stmt::Block(self.parse_block()?));
        }

        let expr = self.parse_expr()?;
        let span = expr.span;
        self.consume_optional_semicolon();
        Some(Stmt::Expr { expr, span })
    }

    fn parse_let_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwLet, "expected 'let'")?.span;
        let name = self
            .expect(TokenKind::Identifier, "expected variable name")?
            .text;
        let annotation = if self.at(TokenKind::Colon) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.at(TokenKind::Eq) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.consume_optional_semicolon();
        Some(Stmt::Let {
            name,
            annotation,
            value,
            span: Span::new(start.file_id, start.start, self.prev_span().end),
        })
    }

    fn parse_return_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwReturn, "expected 'return'")?.span;
        let value = if self.at_stmt_end() {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.consume_optional_semicolon();
        Some(Stmt::Return {
            value,
            span: Span::new(start.file_id, start.start, self.prev_span().end),
        })
    }

    fn parse_raise_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwRaise, "expected 'raise'")?.span;
        let error = self.parse_expr()?;
        self.consume_optional_semicolon();
        Some(Stmt::Raise {
            error,
            span: Span::new(start.file_id, start.start, self.prev_span().end),
        })
    }

    fn parse_if_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwIf, "expected 'if'")?.span;
        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;

        let else_block = if self.at(TokenKind::KwElse) {
            self.bump();
            if self.at(TokenKind::KwIf) {
                let nested = self.parse_if_stmt()?;
                let nested_span = match &nested {
                    Stmt::If { span, .. } => *span,
                    _ => start,
                };
                Some(Block {
                    statements: vec![nested],
                    span: nested_span,
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        let end = else_block
            .as_ref()
            .map(|b| b.span)
            .unwrap_or(then_block.span);

        Some(Stmt::If {
            condition,
            then_block,
            else_block,
            span: Span::new(start.file_id, start.start, end.end),
        })
    }

    fn parse_while_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwWhile, "expected 'while'")?.span;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        Some(Stmt::While {
            condition,
            body: body.clone(),
            span: Span::new(start.file_id, start.start, body.span.end),
        })
    }

    fn parse_for_stmt(&mut self) -> Option<Stmt> {
        let start = self.expect(TokenKind::KwFor, "expected 'for'")?.span;
        let name = self
            .expect(TokenKind::Identifier, "expected loop variable name")?
            .text;
        self.expect(TokenKind::KwIn, "expected 'in' in for statement")?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;
        Some(Stmt::For {
            name,
            iterable,
            body: body.clone(),
            span: Span::new(start.file_id, start.start, body.span.end),
        })
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Option<Expr> {
        let mut expr = self.parse_logical_or()?;

        let assign_op = match self.current().kind {
            TokenKind::Eq => Some(AssignOp::Assign),
            TokenKind::PlusEq => Some(AssignOp::AddAssign),
            TokenKind::MinusEq => Some(AssignOp::SubAssign),
            TokenKind::StarEq => Some(AssignOp::MulAssign),
            TokenKind::SlashEq => Some(AssignOp::DivAssign),
            _ => None,
        };

        if let Some(op) = assign_op {
            self.bump();
            let value = self.parse_assignment()?;
            let span = Span::new(expr.span.file_id, expr.span.start, value.span.end);
            expr = Expr {
                kind: ExprKind::Assign {
                    op,
                    target: Box::new(expr),
                    value: Box::new(value),
                },
                span,
            };
        }

        Some(expr)
    }

    fn parse_logical_or(&mut self) -> Option<Expr> {
        self.parse_left_assoc(Parser::parse_logical_and, &[TokenKind::OrOr])
    }

    fn parse_logical_and(&mut self) -> Option<Expr> {
        self.parse_left_assoc(Parser::parse_equality, &[TokenKind::AndAnd])
    }

    fn parse_equality(&mut self) -> Option<Expr> {
        self.parse_left_assoc(
            Parser::parse_comparison,
            &[TokenKind::EqEq, TokenKind::NotEq],
        )
    }

    fn parse_comparison(&mut self) -> Option<Expr> {
        self.parse_left_assoc(
            Parser::parse_range,
            &[TokenKind::Lt, TokenKind::Lte, TokenKind::Gt, TokenKind::Gte],
        )
    }

    fn parse_range(&mut self) -> Option<Expr> {
        self.parse_left_assoc(Parser::parse_additive, &[TokenKind::DotDot])
    }

    fn parse_additive(&mut self) -> Option<Expr> {
        self.parse_left_assoc(
            Parser::parse_multiplicative,
            &[TokenKind::Plus, TokenKind::Minus],
        )
    }

    fn parse_multiplicative(&mut self) -> Option<Expr> {
        self.parse_left_assoc(
            Parser::parse_unary,
            &[TokenKind::Star, TokenKind::Slash, TokenKind::Percent],
        )
    }

    fn parse_left_assoc(
        &mut self,
        sub: fn(&mut Parser) -> Option<Expr>,
        operators: &[TokenKind],
    ) -> Option<Expr> {
        let mut lhs = sub(self)?;
        while operators.contains(&self.current().kind) {
            let op_token = self.bump();
            let rhs = sub(self)?;
            let op = token_to_binary_op(op_token.kind)?;
            let span = Span::new(lhs.span.file_id, lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Some(lhs)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        if self.at(TokenKind::Bang) {
            let tok = self.bump();
            let expr = self.parse_unary()?;
            return Some(Expr {
                span: Span::new(tok.span.file_id, tok.span.start, expr.span.end),
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
            });
        }

        if self.at(TokenKind::Minus) {
            let tok = self.bump();
            let expr = self.parse_unary()?;
            return Some(Expr {
                span: Span::new(tok.span.file_id, tok.span.start, expr.span.end),
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.at(TokenKind::LParen) {
                let call_start = expr.span;
                self.bump();
                let mut args = Vec::new();
                while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                    args.push(self.parse_expr()?);
                    if self.at(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let end = self
                    .expect(TokenKind::RParen, "expected ')' after call")?
                    .span;
                expr = Expr {
                    span: Span::new(call_start.file_id, call_start.start, end.end),
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                };
                continue;
            }

            if self.at(TokenKind::Dot) {
                self.bump();
                let field = self
                    .expect(TokenKind::Identifier, "expected field name after '.'")?
                    .text;
                let span = Span::new(expr.span.file_id, expr.span.start, self.prev_span().end);
                expr = Expr {
                    span,
                    kind: ExprKind::Member {
                        base: Box::new(expr),
                        field,
                    },
                };
                continue;
            }

            if self.at(TokenKind::LBracket) {
                self.bump();
                let index = self.parse_expr()?;
                let end =
                    self.expect(TokenKind::RBracket, "expected ']' after index expression")?;
                let span = Span::new(expr.span.file_id, expr.span.start, end.span.end);
                expr = Expr {
                    span,
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                };
                continue;
            }

            if self.at(TokenKind::PlusPlus) {
                let op = self.bump();
                let span = Span::new(expr.span.file_id, expr.span.start, op.span.end);
                expr = Expr {
                    span,
                    kind: ExprKind::PostIncrement {
                        target: Box::new(expr),
                    },
                };
                continue;
            }

            break;
        }

        Some(expr)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Identifier => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Name(token.text),
                    span: token.span,
                })
            }
            TokenKind::IntLiteral => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::IntLiteral(token.text),
                    span: token.span,
                })
            }
            TokenKind::FloatLiteral => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::FloatLiteral(token.text),
                    span: token.span,
                })
            }
            TokenKind::StringLiteral => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::StringLiteral(unquote(&token.text)),
                    span: token.span,
                })
            }
            TokenKind::KwTrue => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::BoolLiteral(true),
                    span: token.span,
                })
            }
            TokenKind::KwFalse => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::BoolLiteral(false),
                    span: token.span,
                })
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                let end = self.expect(TokenKind::RParen, "expected ')' after expression")?;
                Some(Expr {
                    span: Span::new(token.span.file_id, token.span.start, end.span.end),
                    kind: inner.kind,
                })
            }
            _ => {
                self.diagnostics.error(
                    "PAR005",
                    format!("expected expression, found '{}'", token.text),
                    Some(token.span),
                );
                None
            }
        }
    }

    fn consume_optional_semicolon(&mut self) {
        if self.at(TokenKind::Semicolon) {
            self.bump();
        }
    }

    fn at_stmt_end(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
        )
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            self.diagnostics
                .error("PAR999", message, Some(self.current().span));
            None
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.idx].clone();
        if self.idx < self.tokens.len().saturating_sub(1) {
            self.idx += 1;
        }
        token
    }

    fn current(&self) -> &Token {
        &self.tokens[self.idx]
    }

    fn prev_span(&self) -> Span {
        if self.idx == 0 {
            return self.tokens[0].span;
        }
        self.tokens[self.idx - 1].span
    }
}

fn token_to_binary_op(kind: TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Plus => BinaryOp::Add,
        TokenKind::Minus => BinaryOp::Sub,
        TokenKind::Star => BinaryOp::Mul,
        TokenKind::Slash => BinaryOp::Div,
        TokenKind::Percent => BinaryOp::Rem,
        TokenKind::EqEq => BinaryOp::Eq,
        TokenKind::NotEq => BinaryOp::Ne,
        TokenKind::Lt => BinaryOp::Lt,
        TokenKind::Lte => BinaryOp::Lte,
        TokenKind::Gt => BinaryOp::Gt,
        TokenKind::Gte => BinaryOp::Gte,
        TokenKind::AndAnd => BinaryOp::And,
        TokenKind::OrOr => BinaryOp::Or,
        TokenKind::DotDot => BinaryOp::Range,
        _ => return None,
    })
}

fn unquote(text: &str) -> String {
    text.trim_matches('"').to_string()
}

fn parse_usize_literal(text: &str) -> Option<usize> {
    let normalized: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '_')
        .filter(|c| *c != '_')
        .collect();
    normalized.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    #[test]
    fn parses_basic_control_flow() {
        let src = r#"
fn sum(n: i64) -> i64
    effects(none)
{
    let acc = 0
    let i = 0
    while i < n {
        i += 1
    }
    return acc
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());
        let (module, parse_diags) = parse(tokens);
        assert!(!parse_diags.has_errors());
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parses_generic_collection_types() {
        let src = r#"
fn main() -> i64
    effects(alloc)
{
    let v: vec<i64> = vec.new()
    let m: map<str, i64> = map.new()
    let om: ordered_map<str, i64> = ordered_map.new()
    return v.len() + m.len() + om.len()
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parse(tokens);
        assert!(!parse_diags.has_errors());
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parses_ptr_type_in_signature() {
        let src = r#"
extern "C" fn passthrough(p: ptr<i64>) -> ptr<i64>
    effects(none)
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parse(tokens);
        assert!(!parse_diags.has_errors());
        assert_eq!(module.items.len(), 1);
    }
}
