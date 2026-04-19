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
    Vec(Box<Type>),
    Map(Box<Type>, Box<Type>),
    OrderedMap(Box<Type>, Box<Type>),
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
    pub fields: Vec<StructFieldInfo>,
    pub field_indices: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct StructFieldInfo {
    pub name: String,
    pub ty: Type,
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

        let mut fields = Vec::with_capacity(decl.fields.len());
        let mut field_indices = BTreeMap::new();
        for field in &decl.fields {
            if field_indices.contains_key(&field.name) {
                self.diagnostics.error(
                    "SEM002",
                    format!("duplicate field '{}' in struct '{}'", field.name, decl.name),
                    Some(field.span),
                );
                continue;
            }

            let lowered_ty = self.lower_type(&field.ty);
            let index = fields.len();
            fields.push(StructFieldInfo {
                name: field.name.clone(),
                ty: lowered_ty,
            });
            field_indices.insert(field.name.clone(), index);
        }

        self.checked.structs.insert(
            decl.name.clone(),
            StructInfo {
                fields,
                field_indices,
            },
        );
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
                    deterministic_context: sig.attrs.iter().any(|a| a == "deterministic"),
                    loop_depth: 0,
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
            TypeSyntax::Generic { name, args } => lower_generic_type(name, args, |arg| self.lower_type(arg)),
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
    deterministic_context: bool,
    loop_depth: usize,
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

                if value.is_none() {
                    if annotated.is_none() {
                        self.diagnostics.error(
                            "SEM070",
                            format!(
                                "variable '{}' requires either an explicit type annotation or an initializer",
                                name
                            ),
                            Some(*span),
                        );
                    } else if let Some(annotation_ty) = annotated.as_ref()
                        && requires_explicit_initializer(annotation_ty)
                    {
                        self.diagnostics.error(
                            "SEM069",
                            format!(
                                "variable '{}' of type {:?} requires an explicit initializer to avoid hidden allocation",
                                name, annotation_ty
                            ),
                            Some(*span),
                        );
                    }
                }

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
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth = self.loop_depth.saturating_sub(1);
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
                    Type::Array {
                        inner,
                        size: Some(_),
                    } => *inner,
                    Type::Vec(inner) => *inner,
                    Type::Str => Type::I32,
                    Type::Array { size: None, .. } => {
                        self.diagnostics.error(
                            "SEM018",
                            "for-loop over unsized array is not supported yet",
                            Some(*span),
                        );
                        Type::Unknown
                    }
                    _ => {
                        self.diagnostics.error(
                            "SEM015",
                            "for-loop iterable must be range, fixed-size array, vec<T>, or str",
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
                self.loop_depth += 1;
                for stmt in &body.statements {
                    self.check_stmt(stmt);
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                self.locals.pop();
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics.error(
                        "SEM016",
                        "'break' is only valid inside loops",
                        Some(*span),
                    );
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics.error(
                        "SEM017",
                        "'continue' is only valid inside loops",
                        Some(*span),
                    );
                }
            }
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

                let mut result_ty = target_ty.clone();
                if matches!(op, AssignOp::Assign)
                    && let ExprKind::Name(name) = &target.kind
                {
                    let refined_ty = merge_inferred_type(&target_ty, &value_ty);
                    if refined_ty != target_ty {
                        self.update_local_type(name, refined_ty.clone());
                        result_ty = refined_ty;
                    }
                }

                if !matches!(target.kind, ExprKind::Name(_)) {
                    self.used_effects.add_effect("mut");
                }

                result_ty
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
                    Type::Vec(inner) => *inner,
                    _ => {
                        self.diagnostics.error(
                            "SEM027",
                            "indexing is only supported on arrays, vectors, and strings",
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
                    if let Some(ty) =
                        self.check_builtin_member_call(module_name, field, &arg_types, span)
                    {
                        return ty;
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

    fn check_builtin_member_call(
        &mut self,
        module_name: &str,
        field: &str,
        arg_types: &[Type],
        span: Span,
    ) -> Option<Type> {
        match (module_name, field) {
            ("io", "out") => {
                if arg_types.len() != 1 {
                    self.diagnostics.error(
                        "SEM035",
                        "io.out expects exactly one argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] == Type::Void {
                    self.diagnostics.error(
                        "SEM035",
                        "io.out argument cannot be void",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("io");
                Some(Type::Void)
            }
            ("str", "concat") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM036",
                        "str.concat expects exactly two string arguments",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] != Type::Str || arg_types[1] != Type::Str {
                    self.diagnostics.error(
                        "SEM036",
                        "str.concat arguments must both be of type str",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("alloc");
                Some(Type::Str)
            }
            ("str", "len") => {
                if arg_types.len() != 1 {
                    self.diagnostics.error(
                        "SEM037",
                        "str.len expects exactly one argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] != Type::Str {
                    self.diagnostics.error(
                        "SEM037",
                        "str.len argument must be of type str",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::I64)
            }
            ("str", "contains") | ("str", "starts_with") | ("str", "ends_with") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM057",
                        format!("str.{} expects exactly two string arguments", field),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] != Type::Str || arg_types[1] != Type::Str {
                    self.diagnostics.error(
                        "SEM057",
                        format!("str.{} arguments must both be of type str", field),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("str", "find") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM057",
                        "str.find expects exactly two string arguments",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] != Type::Str || arg_types[1] != Type::Str {
                    self.diagnostics.error(
                        "SEM057",
                        "str.find arguments must both be of type str",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::I64)
            }
            ("str", "slice") => {
                if arg_types.len() != 3 {
                    self.diagnostics.error(
                        "SEM057",
                        "str.slice expects arguments (str, start, len)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if arg_types[0] != Type::Str
                    || !arg_types[1].is_integral()
                    || !arg_types[2].is_integral()
                {
                    self.diagnostics.error(
                        "SEM057",
                        "str.slice expects arguments (str, integral start, integral len)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("alloc");
                Some(Type::Str)
            }
            ("vec", "new") => {
                if !arg_types.is_empty() {
                    self.diagnostics.error(
                        "SEM038",
                        "vec.new expects no arguments",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("alloc");
                Some(Type::Vec(Box::new(Type::Unknown)))
            }
            ("vec", "new_i64") => {
                if !arg_types.is_empty() {
                    self.diagnostics.error(
                        "SEM038",
                        "vec.new_i64 expects no arguments",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("alloc");
                Some(Type::Vec(Box::new(Type::I64)))
            }
            ("vec", "with_capacity") => {
                if arg_types.len() != 1 || !arg_types[0].is_integral() {
                    self.diagnostics.error(
                        "SEM038",
                        "vec.with_capacity expects one integral capacity argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                self.used_effects.add_effect("alloc");
                Some(Type::Vec(Box::new(Type::Unknown)))
            }
            ("vec", "push") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM039",
                        "vec.push expects arguments (vec<T>, T)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Vec(inner) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM039",
                        "vec.push first argument must be vec<T>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                let resolved_elem_ty = if is_unknown_type(inner.as_ref()) {
                    arg_types[1].clone()
                } else if self.is_assignable(inner.as_ref(), &arg_types[1]) {
                    inner.as_ref().clone()
                } else {
                    self.diagnostics.error(
                        "SEM039",
                        format!(
                            "vec.push element type mismatch: vec<{:?}> cannot accept {:?}",
                            inner,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                self.used_effects.add_effect("alloc");
                Some(Type::Vec(Box::new(resolved_elem_ty)))
            }
            ("vec", "get") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM055",
                        "vec.get expects arguments (vec<T>, index)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if !arg_types[1].is_integral() {
                    self.diagnostics.error(
                        "SEM055",
                        "vec.get expects an integral index",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Vec(inner) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM055",
                        "vec.get first argument must be vec<T>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                Some(inner.as_ref().clone())
            }
            ("vec", "remove") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.remove expects arguments (vec<T>, index)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if !arg_types[1].is_integral() {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.remove expects an integral index",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Vec(inner) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.remove first argument must be vec<T>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                Some(Type::Vec(inner.clone()))
            }
            ("vec", "clear") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::Vec(_)) {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.clear expects one vec<T> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(arg_types[0].clone())
            }
            ("vec", "is_empty") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::Vec(_)) {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.is_empty expects one vec<T> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("vec", "len") => {
                if arg_types.len() != 1 {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.len expects exactly one vec<T> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                if !matches!(&arg_types[0], Type::Vec(_)) {
                    self.diagnostics.error(
                        "SEM056",
                        "vec.len expects exactly one vec<T> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::I64)
            }
            ("map", _) if self.deterministic_context => {
                self.diagnostics.error(
                    "SEM058",
                    "map<K, V> is not allowed in deterministic contexts; use ordered_map<K, V>",
                    Some(span),
                );
                Some(Type::Unknown)
            }
            ("map", "new") => {
                if !arg_types.is_empty() {
                    self.diagnostics.error("SEM059", "map.new expects no arguments", Some(span));
                    return Some(Type::Unknown);
                }
                self.used_effects.add_effect("alloc");
                Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)))
            }
            ("map", "with_capacity") => {
                if arg_types.len() != 1 || !arg_types[0].is_integral() {
                    self.diagnostics.error(
                        "SEM059",
                        "map.with_capacity expects one integral capacity argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }
                self.used_effects.add_effect("alloc");
                Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)))
            }
            ("map", "put") => {
                if arg_types.len() != 3 {
                    self.diagnostics.error(
                        "SEM060",
                        "map.put expects arguments (map<K, V>, K, V)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Map(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM060",
                        "map.put first argument must be map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                let resolved_key_ty = if is_unknown_type(key_ty.as_ref()) {
                    arg_types[1].clone()
                } else if self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    key_ty.as_ref().clone()
                } else {
                    self.diagnostics.error(
                        "SEM060",
                        format!(
                            "map.put key type mismatch: map<{:?}, _> cannot accept {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(&resolved_key_ty) && !is_hashable_map_key_type(&resolved_key_ty) {
                    self.diagnostics.error(
                        "SEM060",
                        format!(
                            "map<K, V> key type {:?} is not currently hashable",
                            resolved_key_ty
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let resolved_value_ty = if is_unknown_type(value_ty.as_ref()) {
                    arg_types[2].clone()
                } else if self.is_assignable(value_ty.as_ref(), &arg_types[2]) {
                    value_ty.as_ref().clone()
                } else {
                    self.diagnostics.error(
                        "SEM060",
                        format!(
                            "map.put value type mismatch: map<_, {:?}> cannot accept {:?}",
                            value_ty,
                            arg_types[2]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                self.used_effects.add_effect("alloc");
                Some(Type::Map(
                    Box::new(resolved_key_ty),
                    Box::new(resolved_value_ty),
                ))
            }
            ("map", "get") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM061",
                        "map.get expects arguments (map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Map(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM061",
                        "map.get first argument must be map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref()) && !self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    self.diagnostics.error(
                        "SEM061",
                        format!(
                            "map.get key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(value_ty.as_ref().clone())
            }
            ("map", "contains") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM062",
                        "map.contains expects arguments (map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Map(key_ty, _) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM062",
                        "map.contains first argument must be map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref()) && !self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    self.diagnostics.error(
                        "SEM062",
                        format!(
                            "map.contains key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("map", "remove") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM063",
                        "map.remove expects arguments (map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::Map(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM063",
                        "map.remove first argument must be map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref())
                    && !self.is_assignable(key_ty.as_ref(), &arg_types[1])
                {
                    self.diagnostics.error(
                        "SEM063",
                        format!(
                            "map.remove key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Map(key_ty.clone(), value_ty.clone()))
            }
            ("map", "clear") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::Map(_, _)) {
                    self.diagnostics.error(
                        "SEM063",
                        "map.clear expects one map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(arg_types[0].clone())
            }
            ("map", "is_empty") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::Map(_, _)) {
                    self.diagnostics.error(
                        "SEM063",
                        "map.is_empty expects one map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("map", "len") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::Map(_, _)) {
                    self.diagnostics.error(
                        "SEM063",
                        "map.len expects one map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::I64)
            }
            ("ordered_map", "new") => {
                if !arg_types.is_empty() {
                    self.diagnostics.error(
                        "SEM064",
                        "ordered_map.new expects no arguments",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }
                self.used_effects.add_effect("alloc");
                Some(Type::OrderedMap(
                    Box::new(Type::Unknown),
                    Box::new(Type::Unknown),
                ))
            }
            ("ordered_map", "put") => {
                if arg_types.len() != 3 {
                    self.diagnostics.error(
                        "SEM065",
                        "ordered_map.put expects arguments (ordered_map<K, V>, K, V)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::OrderedMap(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM065",
                        "ordered_map.put first argument must be ordered_map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                let resolved_key_ty = if is_unknown_type(key_ty.as_ref()) {
                    arg_types[1].clone()
                } else if self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    key_ty.as_ref().clone()
                } else {
                    self.diagnostics.error(
                        "SEM065",
                        format!(
                            "ordered_map.put key type mismatch: ordered_map<{:?}, _> cannot accept {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(&resolved_key_ty) && !is_orderable_map_key_type(&resolved_key_ty) {
                    self.diagnostics.error(
                        "SEM065",
                        format!(
                            "ordered_map<K, V> key type {:?} is not currently orderable",
                            resolved_key_ty
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let resolved_value_ty = if is_unknown_type(value_ty.as_ref()) {
                    arg_types[2].clone()
                } else if self.is_assignable(value_ty.as_ref(), &arg_types[2]) {
                    value_ty.as_ref().clone()
                } else {
                    self.diagnostics.error(
                        "SEM065",
                        format!(
                            "ordered_map.put value type mismatch: ordered_map<_, {:?}> cannot accept {:?}",
                            value_ty,
                            arg_types[2]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                self.used_effects.add_effect("alloc");
                Some(Type::OrderedMap(
                    Box::new(resolved_key_ty),
                    Box::new(resolved_value_ty),
                ))
            }
            ("ordered_map", "get") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM066",
                        "ordered_map.get expects arguments (ordered_map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::OrderedMap(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM066",
                        "ordered_map.get first argument must be ordered_map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref()) && !self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    self.diagnostics.error(
                        "SEM066",
                        format!(
                            "ordered_map.get key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(value_ty.as_ref().clone())
            }
            ("ordered_map", "contains") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM067",
                        "ordered_map.contains expects arguments (ordered_map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::OrderedMap(key_ty, _) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM067",
                        "ordered_map.contains first argument must be ordered_map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref()) && !self.is_assignable(key_ty.as_ref(), &arg_types[1]) {
                    self.diagnostics.error(
                        "SEM067",
                        format!(
                            "ordered_map.contains key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("ordered_map", "remove") => {
                if arg_types.len() != 2 {
                    self.diagnostics.error(
                        "SEM068",
                        "ordered_map.remove expects arguments (ordered_map<K, V>, K)",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                let Type::OrderedMap(key_ty, value_ty) = &arg_types[0] else {
                    self.diagnostics.error(
                        "SEM068",
                        "ordered_map.remove first argument must be ordered_map<K, V>",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                };

                if !is_unknown_type(key_ty.as_ref())
                    && !self.is_assignable(key_ty.as_ref(), &arg_types[1])
                {
                    self.diagnostics.error(
                        "SEM068",
                        format!(
                            "ordered_map.remove key type mismatch: expected {:?}, got {:?}",
                            key_ty,
                            arg_types[1]
                        ),
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::OrderedMap(key_ty.clone(), value_ty.clone()))
            }
            ("ordered_map", "clear") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::OrderedMap(_, _)) {
                    self.diagnostics.error(
                        "SEM068",
                        "ordered_map.clear expects one ordered_map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(arg_types[0].clone())
            }
            ("ordered_map", "is_empty") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::OrderedMap(_, _)) {
                    self.diagnostics.error(
                        "SEM068",
                        "ordered_map.is_empty expects one ordered_map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::Bool)
            }
            ("ordered_map", "len") => {
                if arg_types.len() != 1 || !matches!(&arg_types[0], Type::OrderedMap(_, _)) {
                    self.diagnostics.error(
                        "SEM068",
                        "ordered_map.len expects one ordered_map<K, V> argument",
                        Some(span),
                    );
                    return Some(Type::Unknown);
                }

                Some(Type::I64)
            }
            _ => None,
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
            BinaryOp::Add => {
                if lhs == &Type::Str && rhs == &Type::Str {
                    Type::Str
                } else if lhs.is_numeric() && rhs.is_numeric() && lhs == rhs {
                    lhs.clone()
                } else {
                    self.diagnostics.error(
                        "SEM041",
                        "'+' operands must be identical numeric types or both str",
                        Some(span),
                    );
                    Type::Unknown
                }
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
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
                if let Some(index) = struct_info.field_indices.get(field) {
                    return struct_info.fields[*index].ty.clone();
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

    fn update_local_type(&mut self, name: &str, refined: Type) {
        for scope in self.locals.iter_mut().rev() {
            if let Some(current) = scope.get_mut(name) {
                *current = refined;
                return;
            }
        }
    }

    fn is_assignable(&self, expected: &Type, actual: &Type) -> bool {
        match (expected, actual) {
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            (Type::Vec(expected_inner), Type::Vec(actual_inner)) => {
                self.is_assignable(expected_inner, actual_inner)
            }
            (Type::Map(expected_k, expected_v), Type::Map(actual_k, actual_v)) => {
                self.is_assignable(expected_k, actual_k) && self.is_assignable(expected_v, actual_v)
            }
            (
                Type::OrderedMap(expected_k, expected_v),
                Type::OrderedMap(actual_k, actual_v),
            ) => {
                self.is_assignable(expected_k, actual_k) && self.is_assignable(expected_v, actual_v)
            }
            (
                Type::Array {
                    inner: expected_inner,
                    size: expected_size,
                },
                Type::Array {
                    inner: actual_inner,
                    size: actual_size,
                },
            ) => {
                (expected_size == actual_size
                    || expected_size.is_none()
                    || actual_size.is_none())
                    && self.is_assignable(expected_inner, actual_inner)
            }
            (
                Type::Ref {
                    region: expected_region,
                    mutable: expected_mutable,
                    inner: expected_inner,
                },
                Type::Ref {
                    region: actual_region,
                    mutable: actual_mutable,
                    inner: actual_inner,
                },
            ) => {
                expected_region == actual_region
                    && expected_mutable == actual_mutable
                    && self.is_assignable(expected_inner, actual_inner)
            }
            _ => expected == actual,
        }
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
        "vec" => Type::Vec(Box::new(Type::Unknown)),
        "map" => Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)),
        "ordered_map" => Type::OrderedMap(Box::new(Type::Unknown), Box::new(Type::Unknown)),
        "vec_i64" => Type::Vec(Box::new(Type::I64)),
        _ => Type::Named(name.to_string()),
    }
}

fn lower_generic_type<F>(name: &str, args: &[TypeSyntax], mut lower: F) -> Type
where
    F: FnMut(&TypeSyntax) -> Type,
{
    match name {
        "vec" if args.len() == 1 => Type::Vec(Box::new(lower(&args[0]))),
        "map" if args.len() == 2 => Type::Map(Box::new(lower(&args[0])), Box::new(lower(&args[1]))),
        "ordered_map" if args.len() == 2 => {
            Type::OrderedMap(Box::new(lower(&args[0])), Box::new(lower(&args[1])))
        }
        _ => Type::Unknown,
    }
}

fn is_unknown_type(ty: &Type) -> bool {
    matches!(ty, Type::Unknown)
}

fn is_hashable_map_key_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool | Type::I64 | Type::I32 | Type::I8 | Type::U64 | Type::U32 | Type::Str
    )
}

fn is_orderable_map_key_type(ty: &Type) -> bool {
    is_hashable_map_key_type(ty)
}

fn requires_explicit_initializer(ty: &Type) -> bool {
    matches!(ty, Type::Str | Type::Vec(_) | Type::Map(_, _) | Type::OrderedMap(_, _))
}

fn merge_inferred_type(current: &Type, inferred: &Type) -> Type {
    match (current, inferred) {
        (Type::Unknown, other) => other.clone(),
        (other, Type::Unknown) => other.clone(),
        (Type::Vec(current_inner), Type::Vec(inferred_inner)) => Type::Vec(Box::new(
            merge_inferred_type(current_inner.as_ref(), inferred_inner.as_ref()),
        )),
        (Type::Map(current_k, current_v), Type::Map(inferred_k, inferred_v)) => Type::Map(
            Box::new(merge_inferred_type(current_k.as_ref(), inferred_k.as_ref())),
            Box::new(merge_inferred_type(current_v.as_ref(), inferred_v.as_ref())),
        ),
        (
            Type::OrderedMap(current_k, current_v),
            Type::OrderedMap(inferred_k, inferred_v),
        ) => Type::OrderedMap(
            Box::new(merge_inferred_type(current_k.as_ref(), inferred_k.as_ref())),
            Box::new(merge_inferred_type(current_v.as_ref(), inferred_v.as_ref())),
        ),
        _ => current.clone(),
    }
}

fn lower_type_no_ctx(syntax: &TypeSyntax) -> Type {
    match syntax {
        TypeSyntax::Void => Type::Void,
        TypeSyntax::Named(name) => lower_named_type(name),
        TypeSyntax::Generic { name, args } => lower_generic_type(name, args, lower_type_no_ctx),
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

    #[test]
    fn rejects_break_and_continue_outside_loops() {
        let src = r#"
fn bad() -> void
    effects(none)
{
    break
    continue
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (_, sema_diags) = check(&module);
        assert!(sema_diags.has_errors());
        assert!(sema_diags.iter().any(|diag| diag.code == "SEM016"));
        assert!(sema_diags.iter().any(|diag| diag.code == "SEM017"));
    }

    #[test]
    fn rejects_hash_map_in_deterministic_context() {
        let src = r#"
@deterministic
fn stable() -> i64
    effects(alloc)
{
    let m: map<str, i64> = map.new()
    m = map.put(m, "x", 1)
    return map.get(m, "x")
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (_, sema_diags) = check(&module);
        assert!(sema_diags.has_errors());
        assert!(sema_diags.iter().any(|diag| diag.code == "SEM058"));
    }

    #[test]
    fn allows_constructor_inference_via_assignment() {
        let src = r#"
fn main() -> i64
    effects(alloc)
{
    let v = vec.new()
    v = vec.push(v, 7)
    return vec.get(v, 0)
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (_, sema_diags) = check(&module);
        assert!(!sema_diags.has_errors());
    }

    #[test]
    fn requires_initializer_for_alloc_like_locals() {
        let src = r#"
fn main() -> i64
    effects(alloc)
{
    let s: str
    return 0
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (_, sema_diags) = check(&module);
        assert!(sema_diags.has_errors());
        assert!(sema_diags.iter().any(|diag| diag.code == "SEM069"));
    }
}
