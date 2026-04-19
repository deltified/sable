use crate::source::Span;

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Import(ImportDecl),
    Struct(StructDecl),
    Function(FunctionDecl),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeSyntax,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub attrs: Vec<Attribute>,
    pub trailing_attrs: Vec<Attribute>,
    pub name: String,
    pub params: Vec<ParamDecl>,
    pub return_type: TypeSyntax,
    pub effects: EffectSyntax,
    pub body: Option<Block>,
    pub is_extern: bool,
    pub extern_abi: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub ty: TypeSyntax,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<AttrArg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AttrArg {
    Ident(String),
    String(String),
    Number(String),
    KeyValue(String, String),
}

#[derive(Debug, Clone)]
pub struct EffectSyntax {
    pub all: bool,
    pub effects: Vec<String>,
    pub raises: Vec<String>,
}

impl EffectSyntax {
    pub fn all() -> Self {
        Self {
            all: true,
            effects: Vec::new(),
            raises: Vec::new(),
        }
    }

    pub fn none() -> Self {
        Self {
            all: false,
            effects: Vec::new(),
            raises: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSyntax {
    Named(String),
    Generic {
        name: String,
        args: Vec<TypeSyntax>,
    },
    Ref {
        region: Option<String>,
        mutable: bool,
        inner: Box<TypeSyntax>,
    },
    Array {
        inner: Box<TypeSyntax>,
        size: Option<usize>,
    },
    Void,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        annotation: Option<TypeSyntax>,
        value: Option<Expr>,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Raise {
        error: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    For {
        name: String,
        iterable: Expr,
        body: Block,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Expr {
        expr: Expr,
        span: Span,
    },
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Name(String),
    IntLiteral(String),
    FloatLiteral(String),
    StringLiteral(String),
    BoolLiteral(bool),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    PostIncrement {
        target: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Member {
        base: Box<Expr>,
        field: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}
