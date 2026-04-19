use std::collections::{BTreeMap, BTreeSet};

use crate::ast::*;
use crate::diagnostics::Diagnostics;
use crate::source::Span;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Void,
    Bool,
    I64,
    I32,
    I8,
    U64,
    U32,
    F64,
    F32,
    Str,
    Named(String),
    Ref {
        region: Option<String>,
        mutable: bool,
        inner: Box<Type>,
    },
    Array {
        inner: Box<Type>,
        size: Option<usize>,
    },
    Range(Box<Type>),
    Unknown,
}

impl Type {
    fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::I64 | Type::I32 | Type::I8 | Type::U64 | Type::U32 | Type::F64 | Type::F32
        )
    }

    fn is_float(&self) -> bool {
        matches!(self, Type::F64 | Type::F32)
    }

    fn is_integral(&self) -> bool {
        matches!(
            self,
            Type::I64 | Type::I32 | Type::I8 | Type::U64 | Type::U32
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct EffectSet {
    pub all: bool,
    pub effects: BTreeSet<String>,
    pub raises: BTreeSet<String>,
}

impl EffectSet {
    pub fn from_syntax(syntax: &EffectSyntax) -> Self {
        let mut set = Self {
            all: syntax.all,
            effects: BTreeSet::new(),
            raises: BTreeSet::new(),
        };

        for effect in &syntax.effects {
            if effect != "none" && effect != "all" {
                set.effects.insert(effect.clone());
            }
        }
        for err in &syntax.raises {
            set.raises.insert(err.clone());
        }
        set
    }

    pub fn add_effect(&mut self, effect: &str) {
        self.effects.insert(effect.to_string());
    }

    pub fn add_raise(&mut self, err: impl Into<String>) {
        self.raises.insert(err.into());
    }

    pub fn missing_from_declared(&self, declared: &EffectSet) -> (Vec<String>, Vec<String>) {
        if declared.all {
            return (Vec::new(), Vec::new());
        }

        let missing_effects = self
            .effects
            .difference(&declared.effects)
            .cloned()
            .collect::<Vec<_>>();
        let missing_raises = self
            .raises
            .difference(&declared.raises)
            .cloned()
            .collect::<Vec<_>>();

        (missing_effects, missing_raises)
    }
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: BTreeMap<String, Type>,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<Type>,
    pub return_type: Type,
    pub declared_effects: EffectSet,
    pub attrs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CheckedProgram {
    pub functions: BTreeMap<String, FunctionSig>,
    pub structs: BTreeMap<String, StructInfo>,
}

pub fn check(module: &Module) -> (CheckedProgram, Diagnostics) {
    let mut checker = Checker {
        module,
        diagnostics: Diagnostics::new(),
        checked: CheckedProgram::default(),
    };

    checker.collect_top_level();
    checker.check_function_bodies();
    (checker.checked, checker.diagnostics)
}

struct Checker<'a> {
    module: &'a Module,
    diagnostics: Diagnostics,
    checked: CheckedProgram,
}

impl<'a> Checker<'a> {
    fn collect_top_level(&mut self) {
        for item in &self.module.items {
            match item {
                Item::Import(_) => {}
                Item::Struct(s) => self.collect_struct(s),
                Item::Function(f) => self.collect_function_signature(f),
            }
        }
    }

    fn collect_struct(&mut self, decl: &StructDecl) {
        if self.checked.structs.contains_key(&decl.name) {
            self.diagnostics.error(
                "SEM001",
                format!("duplicate struct '{}'", decl.name),
                Some(decl.span),
            );
            return;
        }

        let mut fields = BTreeMap::new();
        for field in &decl.fields {
            if fields.contains_key(&field.name) {
                self.diagnostics.error(
                    "SEM002",
                    format!("duplicate field '{}' in struct '{}'", field.name, decl.name),
                    Some(field.span),
                );
                continue;
            }
            fields.insert(field.name.clone(), self.lower_type(&field.ty));
        }

        self.checked
            .structs
            .insert(decl.name.clone(), StructInfo { fields });
    }

    fn collect_function_signature(&mut self, decl: &FunctionDecl) {
        if self.checked.functions.contains_key(&decl.name) {
            self.diagnostics.error(
                "SEM003",
                format!("duplicate function '{}'", decl.name),
                Some(decl.span),
            );
            return;
        }

        let params = decl
            .params
            .iter()
            .map(|p| self.lower_type(&p.ty))
            .collect::<Vec<_>>();
        let return_type = self.lower_type(&decl.return_type);
        let declared_effects = EffectSet::from_syntax(&decl.effects);

        let attrs = decl
            .attrs
            .iter()
            .chain(decl.trailing_attrs.iter())
            .map(|a| a.name.clone())
            .collect::<Vec<_>>();

        self.checked.functions.insert(
            decl.name.clone(),
            FunctionSig {
                params,
                return_type,
                declared_effects,
                attrs,
            },
        );
    }

    fn check_function_bodies(&mut self) {
        for item in &self.module.items {
            let Item::Function(function) = item else {
                continue;
            };
            if function.body.is_none() {
                continue;
            }

            let Some(sig) = self.checked.functions.get(&function.name).cloned() else {
                continue;
            };

            let used_effects = {
                let mut fn_checker = FunctionChecker {
                    diagnostics: &mut self.diagnostics,
                    functions: &self.checked.functions,
                    structs: &self.checked.structs,
                    locals: vec![BTreeMap::new()],
                    declared_return: sig.return_type.clone(),
                    used_effects: EffectSet::default(),
                    current_function: function.name.clone(),
                };

                for (param_decl, ty) in function.params.iter().zip(sig.params.iter()) {
                    fn_checker
                        .locals
                        .last_mut()
                        .expect("scope exists")
                        .insert(param_decl.name.clone(), ty.clone());
                }

                if let Some(body) = &function.body {
                    fn_checker.check_block(body);
                }

                fn_checker.used_effects
            };

            let (missing_effects, missing_raises) =
                used_effects.missing_from_declared(&sig.declared_effects);

            if !missing_effects.is_empty() || !missing_raises.is_empty() {
                self.diagnostics.error(
                    "SEM004",
                    format!(
                        "function '{}' uses undeclared effects: [{}] [{}]",
                        function.name,
                        missing_effects.join(", "),
                        missing_raises
                            .iter()
                            .map(|e| format!("raise({e})"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some(function.span),
                );
            }

            let deterministic = sig.attrs.iter().any(|a| a == "deterministic");
            if deterministic
                && (used_effects.effects.contains("io") || used_effects.effects.contains("unsafe"))
            {
                self.diagnostics.error(
                    "SEM005",
                    format!(
                        "deterministic function '{}' cannot use io or unsafe effects",
                        function.name
                    ),
                    Some(function.span),
                );
            }
        }
    }

    fn lower_type(&mut self, syntax: &TypeSyntax) -> Type {
        match syntax {
            TypeSyntax::Void => Type::Void,
            TypeSyntax::Named(name) => lower_named_type(name),
            TypeSyntax::Ref {
                region,
                mutable,
                inner,
            } => Type::Ref {
                region: region.clone(),
                mutable: *mutable,
                inner: Box::new(self.lower_type(inner)),
            },
            TypeSyntax::Array { inner, size } => Type::Array {
                inner: Box::new(self.lower_type(inner)),
                size: *size,
            },
        }
    }
}

struct FunctionChecker<'a> {
    diagnostics: &'a mut Diagnostics,
    functions: &'a BTreeMap<String, FunctionSig>,
    structs: &'a BTreeMap<String, StructInfo>,
    locals: Vec<BTreeMap<String, Type>>,
    declared_return: Type,
    used_effects: EffectSet,
    current_function: String,
}

impl<'a> FunctionChecker<'a> {
    fn check_block(&mut self, block: &Block) {
        self.locals.push(BTreeMap::new());
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        self.locals.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                annotation,
                value,
                span,
            } => {
                let annotated = annotation.as_ref().map(lower_type_no_ctx);
                let value_ty = value
                    .as_ref()
                    .map(|expr| self.check_expr(expr))
                    .unwrap_or(Type::Unknown);

                let final_ty = if let Some(annotation_ty) = annotated {
                    if value.is_some() && !self.is_assignable(&annotation_ty, &value_ty) {
                        self.diagnostics.error(
                            "SEM010",
                            format!(
                                "cannot assign value of type {:?} to variable '{}' of type {:?}",
                                value_ty, name, annotation_ty
                            ),
                            Some(*span),
                        );
                    }
                    annotation_ty
                } else {
                    value_ty
                };

                if self
                    .locals
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    self.diagnostics.error(
                        "SEM011",
                        format!("duplicate local variable '{}'", name),
                        Some(*span),
                    );
                }

                self.locals
                    .last_mut()
                    .expect("scope exists")
                    .insert(name.clone(), final_ty);
            }
            Stmt::Return { value, span } => {
                let ret_ty = value
                    .as_ref()
                    .map(|v| self.check_expr(v))
                    .unwrap_or(Type::Void);
                if !self.is_assignable(&self.declared_return, &ret_ty) {
                    self.diagnostics.error(
                        "SEM012",
                        format!(
                            "return type mismatch in function '{}': expected {:?}, got {:?}",
                            self.current_function, self.declared_return, ret_ty
                        ),
                        Some(*span),
                    );
                }
            }
            Stmt::Raise { error, .. } => {
                let _ = self.check_expr(error);
                let raised = infer_raised_error_name(error);
                self.used_effects.add_raise(raised);
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                let cond_ty = self.check_expr(condition);
                if cond_ty != Type::Bool {
                    self.diagnostics
                        .error("SEM013", "if condition must be bool", Some(*span));
                }
                self.check_block(then_block);
                if let Some(else_block) = else_block {
                    self.check_block(else_block);
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => {
                let cond_ty = self.check_expr(condition);
                if cond_ty != Type::Bool {
                    self.diagnostics
                        .error("SEM014", "while condition must be bool", Some(*span));
                }
                self.check_block(body);
            }
            Stmt::For {
                name,
                iterable,
                body,
                span,
            } => {
                let iter_ty = self.check_expr(iterable);
                let elem_ty = match iter_ty {
                    Type::Range(inner) => *inner,
                    Type::Array { inner, .. } => *inner,
                    Type::Str => Type::I32,
                    _ => {
                        self.diagnostics.error(
                            "SEM015",
                            "for-loop iterable must be range, array, or string",
                            Some(*span),
                        );
                        Type::Unknown
                    }
                };

                self.locals.push(BTreeMap::new());
                self.locals
                    .last_mut()
                    .expect("scope exists")
                    .insert(name.clone(), elem_ty);
                for stmt in &body.statements {
                    self.check_stmt(stmt);
                }
                self.locals.pop();
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Block(block) => self.check_block(block),
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Name(name) => self
                .lookup_local(name)
                .or_else(|| {
                    self.functions
                        .get(name)
                        .map(|_| Type::Named("fn".to_string()))
                })
                .unwrap_or_else(|| {
                    self.diagnostics.error(
                        "SEM020",
                        format!("unknown identifier '{}'", name),
                        Some(expr.span),
                    );
                    Type::Unknown
                }),
            ExprKind::IntLiteral(value) => infer_int_type(value),
            ExprKind::FloatLiteral(value) => infer_float_type(value),
            ExprKind::StringLiteral(_) => Type::Str,
            ExprKind::BoolLiteral(_) => Type::Bool,
            ExprKind::Unary { op, expr: inner } => {
                let inner_ty = self.check_expr(inner);
                match op {
                    UnaryOp::Neg => {
                        if !inner_ty.is_numeric() {
                            self.diagnostics.error(
                                "SEM021",
                                "unary '-' requires numeric operand",
                                Some(expr.span),
                            );
                            Type::Unknown
                        } else {
                            inner_ty
                        }
                    }
                    UnaryOp::Not => {
                        if inner_ty != Type::Bool {
                            self.diagnostics.error(
                                "SEM022",
                                "unary '!' requires bool operand",
                                Some(expr.span),
                            );
                            Type::Unknown
                        } else {
                            Type::Bool
                        }
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs);
                let rhs_ty = self.check_expr(rhs);
                self.check_binary(*op, &lhs_ty, &rhs_ty, expr.span)
            }
            ExprKind::Assign { op, target, value } => {
                let target_ty = self.lvalue_type(target);
                let value_ty = self.check_expr(value);
                if !self.is_assignable(&target_ty, &value_ty) {
                    self.diagnostics.error(
                        "SEM023",
                        format!(
                            "assignment type mismatch: cannot assign {:?} to {:?}",
                            value_ty, target_ty
                        ),
                        Some(expr.span),
                    );
                }

                if !matches!(op, AssignOp::Assign) && !target_ty.is_numeric() {
                    self.diagnostics.error(
                        "SEM024",
                        "compound assignment requires numeric target",
                        Some(expr.span),
                    );
                }

                if !matches!(target.kind, ExprKind::Name(_)) {
                    self.used_effects.add_effect("mut");
                }

                target_ty
            }
            ExprKind::PostIncrement { target } => {
                let target_ty = self.lvalue_type(target);
                if !target_ty.is_integral() {
                    self.diagnostics.error(
                        "SEM025",
                        "post-increment requires integral target",
                        Some(expr.span),
                    );
                }
                if !matches!(target.kind, ExprKind::Name(_)) {
                    self.used_effects.add_effect("mut");
                }
                target_ty
            }
            ExprKind::Call { callee, args } => self.check_call(callee, args, expr.span),
            ExprKind::Member { base, field } => {
                let base_ty = self.check_expr(base);
                self.member_type(&base_ty, field, expr.span)
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base);
                let index_ty = self.check_expr(index);
                if !index_ty.is_integral() {
                    self.diagnostics.error(
                        "SEM026",
                        "index expression must be integral",
                        Some(expr.span),
                    );
                }
                match base_ty {
                    Type::Array { inner, .. } => *inner,
                    Type::Str => Type::I32,
                    _ => {
                        self.diagnostics.error(
                            "SEM027",
                            "indexing is only supported on arrays and strings",
                            Some(expr.span),
                        );
                        Type::Unknown
                    }
                }
            }
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Type {
        let arg_types = args
            .iter()
            .map(|arg| self.check_expr(arg))
            .collect::<Vec<_>>();

        match &callee.kind {
            ExprKind::Name(name) => {
                let Some(sig) = self.functions.get(name) else {
                    self.diagnostics.error(
                        "SEM030",
                        format!("call to unknown function '{}'", name),
                        Some(span),
                    );
                    return Type::Unknown;
                };

                if sig.params.len() != args.len() {
                    self.diagnostics.error(
                        "SEM031",
                        format!(
                            "function '{}' expects {} arguments, got {}",
                            name,
                            sig.params.len(),
                            args.len()
                        ),
                        Some(span),
                    );
                }

                for (index, (expected, actual)) in
                    sig.params.iter().zip(arg_types.iter()).enumerate()
                {
                    if !self.is_assignable(expected, actual) {
                        self.diagnostics.error(
                            "SEM034",
                            format!(
                                "argument {} to '{}' has type {:?}, expected {:?}",
                                index, name, actual, expected
                            ),
                            Some(span),
                        );
                    }
                }

                for effect in &sig.declared_effects.effects {
                    self.used_effects.add_effect(effect);
                }
                for err in &sig.declared_effects.raises {
                    self.used_effects.add_raise(err.clone());
                }

                sig.return_type.clone()
            }
            ExprKind::Member { base, field } => {
                if let ExprKind::Name(module_name) = &base.kind {
                    if module_name == "io" && field == "out" {
                        self.used_effects.add_effect("io");
                        return Type::Void;
                    }
                }

                self.diagnostics.error(
                    "SEM032",
                    "unsupported call target; only direct function calls are currently supported",
                    Some(span),
                );
                Type::Unknown
            }
            _ => {
                self.diagnostics
                    .error("SEM033", "unsupported call target expression", Some(span));
                Type::Unknown
            }
        }
    }

    fn check_binary(&mut self, op: BinaryOp, lhs: &Type, rhs: &Type, span: Span) -> Type {
        match op {
            BinaryOp::Range => {
                if lhs.is_integral() && rhs.is_integral() {
                    Type::Range(Box::new(lhs.clone()))
                } else {
                    self.diagnostics.error(
                        "SEM040",
                        "range operator '..' requires integral operands",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if lhs.is_numeric() && rhs.is_numeric() && lhs == rhs {
                    lhs.clone()
                } else {
                    self.diagnostics.error(
                        "SEM041",
                        "arithmetic operands must have identical numeric types",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                if lhs == rhs {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        "SEM042",
                        "equality operands must have the same type",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
            BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte => {
                if lhs.is_numeric() && rhs.is_numeric() && lhs == rhs {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        "SEM043",
                        "comparison operands must have identical numeric types",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if lhs == &Type::Bool && rhs == &Type::Bool {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        "SEM044",
                        "logical operators require bool operands",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
        }
    }

    fn lvalue_type(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Name(name) => self.lookup_local(name).unwrap_or_else(|| {
                self.diagnostics.error(
                    "SEM050",
                    format!("assignment target '{}' is not a local variable", name),
                    Some(expr.span),
                );
                Type::Unknown
            }),
            ExprKind::Member { base, field } => {
                let base_ty = self.check_expr(base);
                self.member_type(&base_ty, field, expr.span)
            }
            ExprKind::Index { base, .. } => {
                let base_ty = self.check_expr(base);
                match base_ty {
                    Type::Array { inner, .. } => *inner,
                    _ => {
                        self.diagnostics.error(
                            "SEM051",
                            "index assignment target must be an array",
                            Some(expr.span),
                        );
                        Type::Unknown
                    }
                }
            }
            _ => {
                self.diagnostics
                    .error("SEM052", "invalid assignment target", Some(expr.span));
                Type::Unknown
            }
        }
    }

    fn member_type(&mut self, base_ty: &Type, field: &str, span: Span) -> Type {
        let base_ty = match base_ty {
            Type::Ref { inner, .. } => inner.as_ref(),
            other => other,
        };

        if let Type::Named(name) = base_ty {
            if let Some(struct_info) = self.structs.get(name) {
                if let Some(ty) = struct_info.fields.get(field) {
                    return ty.clone();
                }
                self.diagnostics.error(
                    "SEM053",
                    format!("struct '{}' has no field '{}'", name, field),
                    Some(span),
                );
                return Type::Unknown;
            }
        }

        if matches!(base_ty, Type::Unknown) {
            return Type::Unknown;
        }

        self.diagnostics.error(
            "SEM054",
            format!(
                "member access '.{}' is not valid for type {:?}",
                field, base_ty
            ),
            Some(span),
        );
        Type::Unknown
    }

    fn lookup_local(&self, name: &str) -> Option<Type> {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn is_assignable(&self, expected: &Type, actual: &Type) -> bool {
        expected == actual || *expected == Type::Unknown || *actual == Type::Unknown
    }
}

fn lower_named_type(name: &str) -> Type {
    match name {
        "void" => Type::Void,
        "bool" => Type::Bool,
        "i64" => Type::I64,
        "i32" => Type::I32,
        "i8" => Type::I8,
        "u64" => Type::U64,
        "u32" => Type::U32,
        "f64" => Type::F64,
        "f32" => Type::F32,
        "str" => Type::Str,
        _ => Type::Named(name.to_string()),
    }
}

fn lower_type_no_ctx(syntax: &TypeSyntax) -> Type {
    match syntax {
        TypeSyntax::Void => Type::Void,
        TypeSyntax::Named(name) => lower_named_type(name),
        TypeSyntax::Ref {
            region,
            mutable,
            inner,
        } => Type::Ref {
            region: region.clone(),
            mutable: *mutable,
            inner: Box::new(lower_type_no_ctx(inner)),
        },
        TypeSyntax::Array { inner, size } => Type::Array {
            inner: Box::new(lower_type_no_ctx(inner)),
            size: *size,
        },
    }
}

fn infer_int_type(value: &str) -> Type {
    if value.ends_with("us") {
        return Type::U32;
    }
    if value.ends_with('u') {
        return Type::U64;
    }
    if value.ends_with('s') {
        return Type::I32;
    }
    if value.ends_with('c') {
        return Type::I8;
    }
    Type::I64
}

fn infer_float_type(value: &str) -> Type {
    if value.ends_with('f') {
        return Type::F32;
    }
    Type::F64
}

fn infer_raised_error_name(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Name(name) => name.clone(),
        ExprKind::Member { base, .. } => match &base.kind {
            ExprKind::Name(name) => name.clone(),
            _ => "UnknownError".to_string(),
        },
        _ => "UnknownError".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{lexer, parser};

    use super::*;

    #[test]
    fn rejects_undeclared_transitive_effects() {
        let src = r#"
fn io_leaf() -> i64
    effects(io)
{
    io.out(1)
    return 1
}

fn caller() -> i64
    effects(none)
{
    return io_leaf()
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (_, sema_diags) = check(&module);
        assert!(sema_diags.has_errors());
    }
}
