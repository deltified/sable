use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};

use crate::ast::{
    AssignOp, BinaryOp, Block, Expr, ExprKind, FunctionDecl, Module, Stmt, TypeSyntax, UnaryOp,
};
use crate::sema::{CheckedProgram, EffectSet, Type};

pub type BlockId = usize;

#[derive(Debug, Clone, Default)]
pub struct MirProgram {
    pub structs: BTreeMap<String, MirStruct>,
    pub functions: BTreeMap<String, MirFunction>,
}

#[derive(Debug, Clone)]
pub struct MirStruct {
    pub name: String,
    pub fields: Vec<MirStructField>,
    pub field_indices: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct MirStructField {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<MirParam>,
    pub return_type: Type,
    pub effects: EffectSet,
    pub attrs: Vec<String>,
    pub entry: BlockId,
    pub blocks: Vec<MirBlock>,
}

#[derive(Debug, Clone)]
pub struct MirParam {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct MirBlock {
    pub id: BlockId,
    pub label: String,
    pub instructions: Vec<MirInstruction>,
    pub terminator: MirTerminator,
}

#[derive(Debug, Clone)]
pub struct MirInstruction {
    pub dest: Option<String>,
    pub ty: Type,
    pub effects: BTreeSet<String>,
    pub kind: MirInstructionKind,
}

#[derive(Debug, Clone)]
pub enum MirInstructionKind {
    Copy(MirOperand),
    Unary {
        op: UnaryOp,
        operand: MirOperand,
    },
    Binary {
        op: BinaryOp,
        lhs: MirOperand,
        rhs: MirOperand,
    },
    Call {
        callee: String,
        args: Vec<MirOperand>,
    },
    MemberLoad {
        base: MirOperand,
        field: String,
    },
    IndexLoad {
        base: MirOperand,
        index: MirOperand,
    },
}

#[derive(Debug, Clone)]
pub enum MirTerminator {
    Goto(BlockId),
    Branch {
        cond: MirOperand,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    Return(Option<MirOperand>),
    Unreachable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirOperand {
    Local(String),
    Const(MirConst),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirConst {
    Int(i64, Type),
    Float(f64, Type),
    Bool(bool),
    Str(String),
}

pub fn lower(module: &Module, checked: &CheckedProgram) -> Result<MirProgram> {
    let mut program = MirProgram::default();

    for (name, struct_info) in &checked.structs {
        let fields = struct_info
            .fields
            .iter()
            .map(|field| MirStructField {
                name: field.name.clone(),
                ty: field.ty.clone(),
            })
            .collect::<Vec<_>>();

        program.structs.insert(
            name.clone(),
            MirStruct {
                name: name.clone(),
                fields,
                field_indices: struct_info.field_indices.clone(),
            },
        );
    }

    for item in &module.items {
        let crate::ast::Item::Function(function) = item else {
            continue;
        };

        let Some(signature) = checked.functions.get(&function.name) else {
            continue;
        };

        let mir_function = MirLowerer::new(checked, function, signature).lower_function()?;
        program.functions.insert(function.name.clone(), mir_function);
    }

    Ok(program)
}

pub fn optimize(program: &mut MirProgram) {
    for function in program.functions.values_mut() {
        constant_fold_function(function);
        eliminate_dead_branches(function);
    }
}

struct MirLowerer<'a> {
    checked: &'a CheckedProgram,
    ast_function: &'a FunctionDecl,
    signature: &'a crate::sema::FunctionSig,
    blocks: Vec<MirBlockBuilder>,
    next_temp: usize,
    next_local: usize,
    scopes: Vec<BTreeMap<String, String>>,
    local_types: BTreeMap<String, Type>,
    loop_stack: Vec<LoopTargets>,
}

#[derive(Clone, Copy)]
struct LoopTargets {
    break_bb: BlockId,
    continue_bb: BlockId,
}

struct MirBlockBuilder {
    label: String,
    instructions: Vec<MirInstruction>,
    terminator: Option<MirTerminator>,
}

#[derive(Debug, Clone)]
enum LoweredValue {
    Scalar(TypedOperand),
    Range {
        start: TypedOperand,
        end: TypedOperand,
        elem_ty: Type,
    },
    Void,
}

#[derive(Debug, Clone)]
struct TypedOperand {
    operand: MirOperand,
    ty: Type,
}

impl<'a> MirLowerer<'a> {
    fn new(
        checked: &'a CheckedProgram,
        ast_function: &'a FunctionDecl,
        signature: &'a crate::sema::FunctionSig,
    ) -> Self {
        Self {
            checked,
            ast_function,
            signature,
            blocks: Vec::new(),
            next_temp: 0,
            next_local: 0,
            scopes: vec![BTreeMap::new()],
            local_types: BTreeMap::new(),
            loop_stack: Vec::new(),
        }
    }

    fn lower_function(mut self) -> Result<MirFunction> {
        if self.ast_function.is_extern || self.ast_function.body.is_none() {
            return Ok(MirFunction {
                name: self.ast_function.name.clone(),
                params: self
                    .ast_function
                    .params
                    .iter()
                    .zip(self.signature.params.iter())
                    .map(|(param, ty)| MirParam {
                        name: param.name.clone(),
                        ty: ty.clone(),
                    })
                    .collect(),
                return_type: self.signature.return_type.clone(),
                effects: self.signature.declared_effects.clone(),
                attrs: self.signature.attrs.clone(),
                entry: 0,
                blocks: Vec::new(),
            });
        }

        let entry = self.new_block("entry");

        for (param, ty) in self
            .ast_function
            .params
            .iter()
            .zip(self.signature.params.iter())
        {
            self.define_local_with_exact_name(&param.name, ty.clone())?;
        }

        let body = self
            .ast_function
            .body
            .as_ref()
            .expect("checked body presence");

        let tail = self.lower_ast_block(body, entry)?;
        if let Some(tail_bb) = tail {
            if self.blocks[tail_bb].terminator.is_none() {
                if self.signature.return_type == Type::Void {
                    self.set_terminator(tail_bb, MirTerminator::Return(None))?;
                } else {
                    bail!(
                        "function '{}' can reach the end without returning a value",
                        self.ast_function.name
                    );
                }
            }
        }

        let blocks = self
            .blocks
            .into_iter()
            .enumerate()
            .map(|(id, block)| MirBlock {
                id,
                label: block.label,
                instructions: block.instructions,
                terminator: block.terminator.unwrap_or(MirTerminator::Unreachable),
            })
            .collect::<Vec<_>>();

        Ok(MirFunction {
            name: self.ast_function.name.clone(),
            params: self
                .ast_function
                .params
                .iter()
                .zip(self.signature.params.iter())
                .map(|(param, ty)| MirParam {
                    name: param.name.clone(),
                    ty: ty.clone(),
                })
                .collect(),
            return_type: self.signature.return_type.clone(),
            effects: self.signature.declared_effects.clone(),
            attrs: self.signature.attrs.clone(),
            entry,
            blocks,
        })
    }

    fn lower_ast_block(&mut self, block: &Block, start_bb: BlockId) -> Result<Option<BlockId>> {
        self.scopes.push(BTreeMap::new());

        let mut current = Some(start_bb);
        for stmt in &block.statements {
            let Some(bb) = current else {
                break;
            };
            current = self.lower_stmt(stmt, bb)?;
        }

        self.scopes.pop();
        Ok(current)
    }

    fn lower_stmt(&mut self, stmt: &Stmt, current_bb: BlockId) -> Result<Option<BlockId>> {
        match stmt {
            Stmt::Let {
                name,
                annotation,
                value,
                ..
            } => {
                let declared_ty = annotation.as_ref().map(lower_type_syntax_no_ctx);
                let lowered_value = if let Some(expr) = value {
                    Some(self.lower_expr(expr, current_bb)?)
                } else {
                    None
                };

                let final_ty = declared_ty
                    .clone()
                    .or_else(|| match &lowered_value {
                        Some(LoweredValue::Scalar(value)) => Some(value.ty.clone()),
                        _ => None,
                    })
                    .unwrap_or(Type::Unknown);

                let destination = self.define_local(name, final_ty.clone())?;

                let init_operand = match lowered_value {
                    Some(LoweredValue::Scalar(value)) => value.operand,
                    Some(LoweredValue::Void) => {
                        return Err(anyhow!(
                            "variable '{}' cannot be initialized from a void expression",
                            name
                        ));
                    }
                    Some(LoweredValue::Range { .. }) => {
                        return Err(anyhow!(
                            "variable '{}' cannot be initialized from a range expression",
                            name
                        ));
                    }
                    None => default_value_for_type(&final_ty),
                };

                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(destination),
                        ty: final_ty,
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Copy(init_operand),
                    },
                )?;

                Ok(Some(current_bb))
            }
            Stmt::Return { value, .. } => {
                let terminator = match value {
                    None => MirTerminator::Return(None),
                    Some(expr) => match self.lower_expr(expr, current_bb)? {
                        LoweredValue::Scalar(value) => MirTerminator::Return(Some(value.operand)),
                        LoweredValue::Void => MirTerminator::Return(None),
                        LoweredValue::Range { .. } => {
                            return Err(anyhow!(
                                "cannot return a range expression directly in '{}'",
                                self.ast_function.name
                            ));
                        }
                    },
                };

                self.set_terminator(current_bb, terminator)?;
                Ok(None)
            }
            Stmt::Raise { error, .. } => {
                let _ = self.lower_expr(error, current_bb)?;
                self.set_terminator(current_bb, MirTerminator::Unreachable)?;
                Ok(None)
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let cond_value = self.lower_expr(condition, current_bb)?;
                let cond = self.expect_scalar(cond_value)?;
                let then_bb = self.new_block("if.then");
                let else_bb = self.new_block("if.else");
                let merge_bb = self.new_block("if.merge");

                self.set_terminator(
                    current_bb,
                    MirTerminator::Branch {
                        cond: cond.operand,
                        then_bb,
                        else_bb,
                    },
                )?;

                let then_tail = self.lower_ast_block(then_block, then_bb)?;
                if let Some(tail) = then_tail {
                    self.set_terminator(tail, MirTerminator::Goto(merge_bb))?;
                }

                let else_tail = if let Some(else_block) = else_block {
                    self.lower_ast_block(else_block, else_bb)?
                } else {
                    Some(else_bb)
                };

                if let Some(tail) = else_tail {
                    self.set_terminator(tail, MirTerminator::Goto(merge_bb))?;
                }

                if then_tail.is_none() && else_tail.is_none() {
                    Ok(None)
                } else {
                    Ok(Some(merge_bb))
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                let cond_bb = self.new_block("while.cond");
                let body_bb = self.new_block("while.body");
                let end_bb = self.new_block("while.end");

                self.set_terminator(current_bb, MirTerminator::Goto(cond_bb))?;

                let cond_value = self.lower_expr(condition, cond_bb)?;
                let cond_operand = self.expect_scalar(cond_value)?;
                self.set_terminator(
                    cond_bb,
                    MirTerminator::Branch {
                        cond: cond_operand.operand,
                        then_bb: body_bb,
                        else_bb: end_bb,
                    },
                )?;

                self.loop_stack.push(LoopTargets {
                    break_bb: end_bb,
                    continue_bb: cond_bb,
                });
                let body_tail = self.lower_ast_block(body, body_bb)?;
                self.loop_stack.pop();

                if let Some(tail) = body_tail {
                    self.set_terminator(tail, MirTerminator::Goto(cond_bb))?;
                }

                Ok(Some(end_bb))
            }
            Stmt::For {
                name,
                iterable,
                body,
                ..
            } => {
                let lowered_iterable = self.lower_expr(iterable, current_bb)?;
                match lowered_iterable {
                    LoweredValue::Range {
                        start,
                        end,
                        elem_ty,
                    } => self.lower_for_range(name, body, current_bb, start, end, elem_ty),
                    LoweredValue::Scalar(iterable) => {
                        self.lower_for_scalar_iterable(name, body, current_bb, iterable)
                    }
                    LoweredValue::Void => Err(anyhow!(
                        "for-loop iterable cannot be a void expression in '{}'",
                        self.ast_function.name
                    )),
                }
            }
            Stmt::Break(_) => {
                let Some(targets) = self.loop_stack.last().copied() else {
                    return Err(anyhow!("'break' used outside of loop"));
                };
                self.set_terminator(current_bb, MirTerminator::Goto(targets.break_bb))?;
                Ok(None)
            }
            Stmt::Continue(_) => {
                let Some(targets) = self.loop_stack.last().copied() else {
                    return Err(anyhow!("'continue' used outside of loop"));
                };
                self.set_terminator(current_bb, MirTerminator::Goto(targets.continue_bb))?;
                Ok(None)
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.lower_expr(expr, current_bb)?;
                Ok(Some(current_bb))
            }
            Stmt::Block(block) => self.lower_ast_block(block, current_bb),
        }
    }

    fn lower_for_range(
        &mut self,
        loop_name: &str,
        body: &Block,
        current_bb: BlockId,
        start: TypedOperand,
        end: TypedOperand,
        elem_ty: Type,
    ) -> Result<Option<BlockId>> {
        let iter_local = self.define_hidden_local("__for_iter", elem_ty.clone())?;
        let end_local = self.define_hidden_local("__for_end", elem_ty.clone())?;

        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(iter_local.clone()),
                ty: elem_ty.clone(),
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(start.operand),
            },
        )?;
        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(end_local.clone()),
                ty: elem_ty.clone(),
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(end.operand),
            },
        )?;

        self.lower_for_loop_core(
            loop_name,
            body,
            current_bb,
            iter_local,
            end_local,
            elem_ty.clone(),
            elem_ty.clone(),
            |lowerer, body_bb, loop_var, iter_local_name| {
                lowerer.emit(
                    body_bb,
                    MirInstruction {
                        dest: Some(loop_var),
                        ty: elem_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Copy(MirOperand::Local(iter_local_name)),
                    },
                )
            },
            int_one_for_type(&elem_ty),
        )
    }

    fn lower_for_scalar_iterable(
        &mut self,
        loop_name: &str,
        body: &Block,
        current_bb: BlockId,
        iterable: TypedOperand,
    ) -> Result<Option<BlockId>> {
        let TypedOperand { operand, ty } = iterable;

        match ty {
            Type::Array {
                inner,
                size: Some(size),
            } => {
                let elem_ty = inner.as_ref().clone();
                let array_local = match operand {
                    MirOperand::Local(local) => local,
                    value => {
                        let temp = self.new_temp(Type::Array {
                            inner: inner.clone(),
                            size: Some(size),
                        });
                        self.emit(
                            current_bb,
                            MirInstruction {
                                dest: Some(temp.clone()),
                                ty: Type::Array {
                                    inner,
                                    size: Some(size),
                                },
                                effects: BTreeSet::new(),
                                kind: MirInstructionKind::Copy(value),
                            },
                        )?;
                        temp
                    }
                };

                self.lower_for_fixed_array(loop_name, body, current_bb, array_local, elem_ty, size)
            }
            Type::Vec(inner) => {
                let elem_ty = inner.as_ref().clone();
                let vec_local = match operand {
                    MirOperand::Local(local) => local,
                    value => {
                        let temp = self.new_temp(Type::Vec(inner.clone()));
                        self.emit(
                            current_bb,
                            MirInstruction {
                                dest: Some(temp.clone()),
                                ty: Type::Vec(inner),
                                effects: BTreeSet::new(),
                                kind: MirInstructionKind::Copy(value),
                            },
                        )?;
                        temp
                    }
                };

                self.lower_for_dynamic_indexable(
                    loop_name,
                    body,
                    current_bb,
                    vec_local,
                    elem_ty,
                    "vec.len",
                )
            }
            Type::Str => {
                let str_local = match operand {
                    MirOperand::Local(local) => local,
                    value => {
                        let temp = self.new_temp(Type::Str);
                        self.emit(
                            current_bb,
                            MirInstruction {
                                dest: Some(temp.clone()),
                                ty: Type::Str,
                                effects: BTreeSet::new(),
                                kind: MirInstructionKind::Copy(value),
                            },
                        )?;
                        temp
                    }
                };

                self.lower_for_dynamic_indexable(
                    loop_name,
                    body,
                    current_bb,
                    str_local,
                    Type::I32,
                    "str.len",
                )
            }
            Type::Array { size: None, .. } => Err(anyhow!(
                "for-loop over unsized arrays is not supported in '{}'",
                self.ast_function.name
            )),
            other => Err(anyhow!(
                "for-loop iterable must be range, fixed-size array, vec<T>, or str, got {:?} in '{}'",
                other,
                self.ast_function.name
            )),
        }
    }

    fn lower_for_dynamic_indexable(
        &mut self,
        loop_name: &str,
        body: &Block,
        current_bb: BlockId,
        base_local: String,
        elem_ty: Type,
        len_builtin: &str,
    ) -> Result<Option<BlockId>> {
        let iter_local = self.define_hidden_local("__for_idx", Type::I64)?;
        let end_local = self.define_hidden_local("__for_len", Type::I64)?;

        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(iter_local.clone()),
                ty: Type::I64,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(MirOperand::Const(MirConst::Int(0, Type::I64))),
            },
        )?;

        let len_temp = self.new_temp(Type::I64);
        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(len_temp.clone()),
                ty: Type::I64,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Call {
                    callee: len_builtin.to_string(),
                    args: vec![MirOperand::Local(base_local.clone())],
                },
            },
        )?;
        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(end_local.clone()),
                ty: Type::I64,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(MirOperand::Local(len_temp)),
            },
        )?;

        self.lower_for_loop_core(
            loop_name,
            body,
            current_bb,
            iter_local,
            end_local,
            elem_ty.clone(),
            Type::I64,
            |lowerer, body_bb, loop_var, iter_local_name| {
                lowerer.emit(
                    body_bb,
                    MirInstruction {
                        dest: Some(loop_var),
                        ty: elem_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::IndexLoad {
                            base: MirOperand::Local(base_local.clone()),
                            index: MirOperand::Local(iter_local_name),
                        },
                    },
                )
            },
            MirOperand::Const(MirConst::Int(1, Type::I64)),
        )
    }

    fn lower_for_fixed_array(
        &mut self,
        loop_name: &str,
        body: &Block,
        current_bb: BlockId,
        array_local: String,
        elem_ty: Type,
        size: usize,
    ) -> Result<Option<BlockId>> {
        let size_i64 = i64::try_from(size).map_err(|_| {
            anyhow!(
                "array length '{}' does not fit i64 in function '{}'",
                size,
                self.ast_function.name
            )
        })?;

        let iter_local = self.define_hidden_local("__for_idx", Type::I64)?;
        let end_local = self.define_hidden_local("__for_len", Type::I64)?;

        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(iter_local.clone()),
                ty: Type::I64,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(MirOperand::Const(MirConst::Int(0, Type::I64))),
            },
        )?;
        self.emit(
            current_bb,
            MirInstruction {
                dest: Some(end_local.clone()),
                ty: Type::I64,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Copy(MirOperand::Const(MirConst::Int(
                    size_i64,
                    Type::I64,
                ))),
            },
        )?;

        self.lower_for_loop_core(
            loop_name,
            body,
            current_bb,
            iter_local,
            end_local,
            elem_ty.clone(),
            Type::I64,
            |lowerer, body_bb, loop_var, iter_local_name| {
                lowerer.emit(
                    body_bb,
                    MirInstruction {
                        dest: Some(loop_var),
                        ty: elem_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::IndexLoad {
                            base: MirOperand::Local(array_local.clone()),
                            index: MirOperand::Local(iter_local_name),
                        },
                    },
                )
            },
            MirOperand::Const(MirConst::Int(1, Type::I64)),
        )
    }

    fn lower_for_loop_core<F>(
        &mut self,
        loop_name: &str,
        body: &Block,
        current_bb: BlockId,
        iter_local: String,
        end_local: String,
        loop_var_ty: Type,
        iter_ty: Type,
        init_loop_var: F,
        step_by: MirOperand,
    ) -> Result<Option<BlockId>>
    where
        F: FnOnce(&mut Self, BlockId, String, String) -> Result<()>,
    {
        let cond_bb = self.new_block("for.cond");
        let body_bb = self.new_block("for.body");
        let step_bb = self.new_block("for.step");
        let end_bb = self.new_block("for.end");

        self.set_terminator(current_bb, MirTerminator::Goto(cond_bb))?;

        let cond_temp = self.new_temp(Type::Bool);
        self.emit(
            cond_bb,
            MirInstruction {
                dest: Some(cond_temp.clone()),
                ty: Type::Bool,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Binary {
                    op: BinaryOp::Lt,
                    lhs: MirOperand::Local(iter_local.clone()),
                    rhs: MirOperand::Local(end_local.clone()),
                },
            },
        )?;
        self.set_terminator(
            cond_bb,
            MirTerminator::Branch {
                cond: MirOperand::Local(cond_temp),
                then_bb: body_bb,
                else_bb: end_bb,
            },
        )?;

        self.scopes.push(BTreeMap::new());
        let loop_var = self.define_local(loop_name, loop_var_ty)?;
        init_loop_var(self, body_bb, loop_var, iter_local.clone())?;

        self.loop_stack.push(LoopTargets {
            break_bb: end_bb,
            continue_bb: step_bb,
        });
        let body_tail = self.lower_ast_block(body, body_bb)?;
        self.loop_stack.pop();
        self.scopes.pop();

        if let Some(tail) = body_tail {
            self.set_terminator(tail, MirTerminator::Goto(step_bb))?;
        }

        self.emit(
            step_bb,
            MirInstruction {
                dest: Some(iter_local.clone()),
                ty: iter_ty,
                effects: BTreeSet::new(),
                kind: MirInstructionKind::Binary {
                    op: BinaryOp::Add,
                    lhs: MirOperand::Local(iter_local),
                    rhs: step_by,
                },
            },
        )?;
        self.set_terminator(step_bb, MirTerminator::Goto(cond_bb))?;

        Ok(Some(end_bb))
    }

    fn lower_expr(&mut self, expr: &Expr, current_bb: BlockId) -> Result<LoweredValue> {
        match &expr.kind {
            ExprKind::Name(name) => {
                let (local, ty) = self.lookup_local(name)?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(local),
                    ty,
                }))
            }
            ExprKind::IntLiteral(text) => {
                let ty = infer_int_type(text);
                let parsed = parse_int_literal(text)?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Const(MirConst::Int(parsed, ty.clone())),
                    ty,
                }))
            }
            ExprKind::FloatLiteral(text) => {
                let ty = infer_float_type(text);
                let parsed = parse_float_literal(text)?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Const(MirConst::Float(parsed, ty.clone())),
                    ty,
                }))
            }
            ExprKind::StringLiteral(text) => Ok(LoweredValue::Scalar(TypedOperand {
                operand: MirOperand::Const(MirConst::Str(text.clone())),
                ty: Type::Str,
            })),
            ExprKind::BoolLiteral(value) => Ok(LoweredValue::Scalar(TypedOperand {
                operand: MirOperand::Const(MirConst::Bool(*value)),
                ty: Type::Bool,
            })),
            ExprKind::Unary { op, expr: inner } => {
                let inner_value = self.lower_expr(inner, current_bb)?;
                let inner = self.expect_scalar(inner_value)?;

                if let Some(constant) = fold_unary_constant(*op, &inner.operand) {
                    let ty = match op {
                        UnaryOp::Neg => inner.ty,
                        UnaryOp::Not => Type::Bool,
                    };
                    return Ok(LoweredValue::Scalar(TypedOperand {
                        operand: MirOperand::Const(constant),
                        ty,
                    }));
                }

                let result_ty = match op {
                    UnaryOp::Neg => inner.ty,
                    UnaryOp::Not => Type::Bool,
                };
                let temp = self.new_temp(result_ty.clone());
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(temp.clone()),
                        ty: result_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Unary {
                            op: *op,
                            operand: inner.operand,
                        },
                    },
                )?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(temp),
                    ty: result_ty,
                }))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                if *op == BinaryOp::Range {
                    let lhs_value = self.lower_expr(lhs, current_bb)?;
                    let lhs = self.expect_scalar(lhs_value)?;
                    let rhs_value = self.lower_expr(rhs, current_bb)?;
                    let rhs = self.expect_scalar(rhs_value)?;
                    return Ok(LoweredValue::Range {
                        start: lhs.clone(),
                        end: rhs,
                        elem_ty: lhs.ty,
                    });
                }

                let lhs_value = self.lower_expr(lhs, current_bb)?;
                let lhs = self.expect_scalar(lhs_value)?;
                let rhs_value = self.lower_expr(rhs, current_bb)?;
                let rhs = self.expect_scalar(rhs_value)?;

                if let Some(constant) = fold_binary_constant(*op, &lhs.operand, &rhs.operand) {
                    let result_ty = binary_result_type(*op, &lhs.ty);
                    return Ok(LoweredValue::Scalar(TypedOperand {
                        operand: MirOperand::Const(constant),
                        ty: result_ty,
                    }));
                }

                if *op == BinaryOp::Add && lhs.ty == Type::Str && rhs.ty == Type::Str {
                    let mut effects = BTreeSet::new();
                    effects.insert("alloc".to_string());
                    let temp = self.new_temp(Type::Str);
                    self.emit(
                        current_bb,
                        MirInstruction {
                            dest: Some(temp.clone()),
                            ty: Type::Str,
                            effects,
                            kind: MirInstructionKind::Call {
                                callee: "str.concat".to_string(),
                                args: vec![lhs.operand, rhs.operand],
                            },
                        },
                    )?;
                    return Ok(LoweredValue::Scalar(TypedOperand {
                        operand: MirOperand::Local(temp),
                        ty: Type::Str,
                    }));
                }

                let result_ty = binary_result_type(*op, &lhs.ty);
                let temp = self.new_temp(result_ty.clone());
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(temp.clone()),
                        ty: result_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Binary {
                            op: *op,
                            lhs: lhs.operand,
                            rhs: rhs.operand,
                        },
                    },
                )?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(temp),
                    ty: result_ty,
                }))
            }
            ExprKind::Assign { op, target, value } => {
                let rhs_value = self.lower_expr(value, current_bb)?;
                let rhs = self.expect_scalar(rhs_value)?;
                let ExprKind::Name(name) = &target.kind else {
                    return Err(anyhow!(
                        "MIR lowering currently supports assignment only to local names"
                    ));
                };

                let (target_local, mut target_ty) = self.lookup_local(name)?;
                if matches!(op, AssignOp::Assign) {
                    let refined_ty = merge_inferred_type(&target_ty, &rhs.ty);
                    if refined_ty != target_ty {
                        self.local_types
                            .insert(target_local.clone(), refined_ty.clone());
                        target_ty = refined_ty;
                    }
                }

                let instruction_kind = match op {
                    AssignOp::Assign => MirInstructionKind::Copy(rhs.operand),
                    AssignOp::AddAssign
                    | AssignOp::SubAssign
                    | AssignOp::MulAssign
                    | AssignOp::DivAssign => {
                        let bin_op = match op {
                            AssignOp::AddAssign => BinaryOp::Add,
                            AssignOp::SubAssign => BinaryOp::Sub,
                            AssignOp::MulAssign => BinaryOp::Mul,
                            AssignOp::DivAssign => BinaryOp::Div,
                            AssignOp::Assign => unreachable!(),
                        };
                        MirInstructionKind::Binary {
                            op: bin_op,
                            lhs: MirOperand::Local(target_local.clone()),
                            rhs: rhs.operand,
                        }
                    }
                };

                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(target_local.clone()),
                        ty: target_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: instruction_kind,
                    },
                )?;

                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(target_local),
                    ty: target_ty,
                }))
            }
            ExprKind::PostIncrement { target } => {
                let ExprKind::Name(name) = &target.kind else {
                    return Err(anyhow!(
                        "MIR lowering currently supports post-increment only on local names"
                    ));
                };

                let (target_local, target_ty) = self.lookup_local(name)?;
                let old_temp = self.new_temp(target_ty.clone());
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(old_temp.clone()),
                        ty: target_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Copy(MirOperand::Local(target_local.clone())),
                    },
                )?;
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(target_local.clone()),
                        ty: target_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::Binary {
                            op: BinaryOp::Add,
                            lhs: MirOperand::Local(target_local),
                            rhs: int_one_for_type(&target_ty),
                        },
                    },
                )?;

                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(old_temp),
                    ty: target_ty,
                }))
            }
            ExprKind::Call { callee, args } => {
                let mut lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(arg, current_bb))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .map(|value| self.expect_scalar(value))
                    .collect::<Result<Vec<_>>>()?;

                let callee_name = match &callee.kind {
                    ExprKind::Name(name) => name.clone(),
                    ExprKind::Member { base, field } => {
                        if let ExprKind::Name(base_name) = &base.kind {
                            if is_static_builtin_member_call(base_name, field) {
                                format!("{base_name}.{field}")
                            } else if is_builtin_namespace_name(base_name) {
                                return Err(anyhow!(
                                    "{}.{} must be called on an instance",
                                    base_name,
                                    field
                                ));
                            } else {
                                let lowered_base = self.lower_expr(base, current_bb)?;
                                let base = self.expect_scalar(lowered_base)?;
                                let Some(namespace) = builtin_namespace_for_type(&base.ty) else {
                                    return Err(anyhow!(
                                        "member call '.{}' is not callable for type {:?}",
                                        field,
                                        base.ty
                                    ));
                                };
                                lowered_args.insert(0, base);
                                format!("{namespace}.{field}")
                            }
                        }
                        else {
                            let lowered_base = self.lower_expr(base, current_bb)?;
                            let base = self.expect_scalar(lowered_base)?;
                            let Some(namespace) = builtin_namespace_for_type(&base.ty) else {
                                return Err(anyhow!(
                                    "member call '.{}' is not callable for type {:?}",
                                    field,
                                    base.ty
                                ));
                            };
                            lowered_args.insert(0, base);
                            format!("{namespace}.{field}")
                        }
                    }
                    _ => return Err(anyhow!("unsupported call target in MIR lowering")),
                };

                        let (return_ty, effects) = if let Some((builtin_ty, builtin_effects)) =
                            self.resolve_builtin_call(&callee_name, &lowered_args)?
                        {
                            (builtin_ty, builtin_effects)
                } else {
                    let Some(sig) = self.checked.functions.get(&callee_name) else {
                        return Err(anyhow!("unknown function '{}'", callee_name));
                    };
                            let mut effects = BTreeSet::new();
                    effects.extend(sig.declared_effects.effects.iter().cloned());
                            (sig.return_type.clone(), effects)
                };

                let args = lowered_args
                    .into_iter()
                    .map(|value| value.operand)
                    .collect::<Vec<_>>();

                if return_ty == Type::Void {
                    self.emit(
                        current_bb,
                        MirInstruction {
                            dest: None,
                            ty: Type::Void,
                            effects,
                            kind: MirInstructionKind::Call {
                                callee: callee_name,
                                args,
                            },
                        },
                    )?;
                    Ok(LoweredValue::Void)
                } else {
                    let temp = self.new_temp(return_ty.clone());
                    self.emit(
                        current_bb,
                        MirInstruction {
                            dest: Some(temp.clone()),
                            ty: return_ty.clone(),
                            effects,
                            kind: MirInstructionKind::Call {
                                callee: callee_name,
                                args,
                            },
                        },
                    )?;
                    Ok(LoweredValue::Scalar(TypedOperand {
                        operand: MirOperand::Local(temp),
                        ty: return_ty,
                    }))
                }
            }
            ExprKind::Member { base, field } => {
                let base_value = self.lower_expr(base, current_bb)?;
                let base = self.expect_scalar(base_value)?;
                let field_ty = self.resolve_member_type(&base.ty, field)?;
                let temp = self.new_temp(field_ty.clone());
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(temp.clone()),
                        ty: field_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::MemberLoad {
                            base: base.operand,
                            field: field.clone(),
                        },
                    },
                )?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(temp),
                    ty: field_ty,
                }))
            }
            ExprKind::Index { base, index } => {
                let base_value = self.lower_expr(base, current_bb)?;
                let base = self.expect_scalar(base_value)?;
                let index_value = self.lower_expr(index, current_bb)?;
                let index = self.expect_scalar(index_value)?;
                let elem_ty = match &base.ty {
                    Type::Array { inner, .. } => inner.as_ref().clone(),
                    Type::Str => Type::I32,
                    Type::Vec(inner) => inner.as_ref().clone(),
                    other => {
                        return Err(anyhow!(
                            "index expression is not supported for type {:?}",
                            other
                        ));
                    }
                };

                let temp = self.new_temp(elem_ty.clone());
                self.emit(
                    current_bb,
                    MirInstruction {
                        dest: Some(temp.clone()),
                        ty: elem_ty.clone(),
                        effects: BTreeSet::new(),
                        kind: MirInstructionKind::IndexLoad {
                            base: base.operand,
                            index: index.operand,
                        },
                    },
                )?;
                Ok(LoweredValue::Scalar(TypedOperand {
                    operand: MirOperand::Local(temp),
                    ty: elem_ty,
                }))
            }
        }
    }

    fn resolve_member_type(&self, base_ty: &Type, field: &str) -> Result<Type> {
        let named = match base_ty {
            Type::Named(name) => Some(name.clone()),
            Type::Ref { inner, .. } => match inner.as_ref() {
                Type::Named(name) => Some(name.clone()),
                _ => None,
            },
            _ => None,
        };

        if let Some(name) = named {
            if let Some(struct_info) = self.checked.structs.get(&name) {
                if let Some(index) = struct_info.field_indices.get(field) {
                    return Ok(struct_info.fields[*index].ty.clone());
                }
            }
        }

        Err(anyhow!(
            "cannot resolve member '{}' for type {:?}",
            field,
            base_ty
        ))
    }

    fn resolve_builtin_call(
        &self,
        callee: &str,
        args: &[TypedOperand],
    ) -> Result<Option<(Type, BTreeSet<String>)>> {
        let mut effects = BTreeSet::new();

        let resolved = match callee {
            "io.out" => {
                if args.len() != 1 {
                    bail!("io.out expects exactly one argument");
                }
                effects.insert("io".to_string());
                Some(Type::Void)
            }
            "str.concat" => {
                if args.len() != 2 || args[0].ty != Type::Str || args[1].ty != Type::Str {
                    bail!("str.concat expects two str arguments");
                }
                effects.insert("alloc".to_string());
                Some(Type::Str)
            }
            "str.len" => {
                if args.len() != 1 || args[0].ty != Type::Str {
                    bail!("str.len expects one str argument");
                }
                Some(Type::I64)
            }
            "str.contains" | "str.starts_with" | "str.ends_with" => {
                if args.len() != 2 || args[0].ty != Type::Str || args[1].ty != Type::Str {
                    bail!("{} expects two str arguments", callee);
                }
                Some(Type::Bool)
            }
            "str.find" => {
                if args.len() != 2 || args[0].ty != Type::Str || args[1].ty != Type::Str {
                    bail!("str.find expects two str arguments");
                }
                Some(Type::I64)
            }
            "str.slice" => {
                if args.len() != 3
                    || args[0].ty != Type::Str
                    || !is_integral_type(&args[1].ty)
                    || !is_integral_type(&args[2].ty)
                {
                    bail!("str.slice expects (str, integral start, integral len)");
                }
                effects.insert("alloc".to_string());
                Some(Type::Str)
            }
            "vec.new" => {
                if !args.is_empty() {
                    bail!("vec.new expects no arguments");
                }
                effects.insert("alloc".to_string());
                Some(Type::Vec(Box::new(Type::Unknown)))
            }
            "vec.new_i64" => {
                if !args.is_empty() {
                    bail!("vec.new_i64 expects no arguments");
                }
                effects.insert("alloc".to_string());
                Some(Type::Vec(Box::new(Type::I64)))
            }
            "vec.with_capacity" => {
                if args.len() != 1 || !is_integral_type(&args[0].ty) {
                    bail!("vec.with_capacity expects one integral capacity argument");
                }
                effects.insert("alloc".to_string());
                Some(Type::Vec(Box::new(Type::Unknown)))
            }
            "vec.push" => {
                if args.len() != 2 {
                    bail!("vec.push expects (vec<T>, T)");
                }

                let Type::Vec(inner) = &args[0].ty else {
                    bail!("vec.push first argument must be vec<T>");
                };

                let resolved_elem_ty = if is_unknown_type(inner.as_ref()) {
                    args[1].ty.clone()
                } else if is_assignable_type(inner.as_ref(), &args[1].ty) {
                    inner.as_ref().clone()
                } else {
                    bail!(
                        "vec.push element type mismatch: vec<{:?}> cannot accept {:?}",
                        inner,
                        args[1].ty
                    );
                };

                effects.insert("alloc".to_string());
                let _ = resolved_elem_ty;
                Some(Type::Void)
            }
            "vec.get" => {
                if args.len() != 2
                    || !is_integral_type(&args[1].ty)
                {
                    bail!("vec.get expects (vec<T>, integral index)");
                }

                let Type::Vec(inner) = &args[0].ty else {
                    bail!("vec.get first argument must be vec<T>");
                };
                Some(inner.as_ref().clone())
            }
            "vec.remove" => {
                if args.len() != 2 || !is_integral_type(&args[1].ty) {
                    bail!("vec.remove expects (vec<T>, integral index)");
                }

                let Type::Vec(inner) = &args[0].ty else {
                    bail!("vec.remove first argument must be vec<T>");
                };
                let _ = inner;
                Some(Type::Void)
            }
            "vec.clear" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Vec(_)) {
                    bail!("vec.clear expects one vec<T> argument");
                }
                Some(Type::Void)
            }
            "vec.is_empty" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Vec(_)) {
                    bail!("vec.is_empty expects one vec<T> argument");
                }
                Some(Type::Bool)
            }
            "vec.len" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Vec(_)) {
                    bail!("vec.len expects one vec<T> argument");
                }
                Some(Type::I64)
            }
            "map.new" => {
                if !args.is_empty() {
                    bail!("map.new expects no arguments");
                }
                effects.insert("alloc".to_string());
                Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)))
            }
            "map.with_capacity" => {
                if args.len() != 1 || !is_integral_type(&args[0].ty) {
                    bail!("map.with_capacity expects one integral capacity argument");
                }
                effects.insert("alloc".to_string());
                Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)))
            }
            "map.put" => {
                if args.len() != 3 {
                    bail!("map.put expects (map<K, V>, K, V)");
                }

                let Type::Map(key_ty, value_ty) = &args[0].ty else {
                    bail!("map.put first argument must be map<K, V>");
                };

                let resolved_key_ty = if is_unknown_type(key_ty.as_ref()) {
                    args[1].ty.clone()
                } else if is_assignable_type(key_ty.as_ref(), &args[1].ty) {
                    key_ty.as_ref().clone()
                } else {
                    bail!(
                        "map.put key type mismatch: map<{:?}, _> cannot accept {:?}",
                        key_ty,
                        args[1].ty
                    );
                };

                if !is_unknown_type(&resolved_key_ty) && !is_hashable_map_key_type(&resolved_key_ty)
                {
                    bail!("map<K, V> key type {:?} is not currently hashable", resolved_key_ty);
                }

                let resolved_value_ty = if is_unknown_type(value_ty.as_ref()) {
                    args[2].ty.clone()
                } else if is_assignable_type(value_ty.as_ref(), &args[2].ty) {
                    value_ty.as_ref().clone()
                } else {
                    bail!(
                        "map.put value type mismatch: map<_, {:?}> cannot accept {:?}",
                        value_ty,
                        args[2].ty
                    );
                };

                effects.insert("alloc".to_string());
                Some(Type::Map(
                    Box::new(resolved_key_ty),
                    Box::new(resolved_value_ty),
                ))
            }
            "map.get" => {
                if args.len() != 2 {
                    bail!("map.get expects (map<K, V>, K)");
                }

                let Type::Map(key_ty, value_ty) = &args[0].ty else {
                    bail!("map.get first argument must be map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref()) && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "map.get key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(value_ty.as_ref().clone())
            }
            "map.contains" => {
                if args.len() != 2 {
                    bail!("map.contains expects (map<K, V>, K)");
                }

                let Type::Map(key_ty, _) = &args[0].ty else {
                    bail!("map.contains first argument must be map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref()) && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "map.contains key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(Type::Bool)
            }
            "map.remove" => {
                if args.len() != 2 {
                    bail!("map.remove expects (map<K, V>, K)");
                }

                let Type::Map(key_ty, value_ty) = &args[0].ty else {
                    bail!("map.remove first argument must be map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref())
                    && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "map.remove key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(Type::Map(key_ty.clone(), value_ty.clone()))
            }
            "map.clear" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Map(_, _)) {
                    bail!("map.clear expects one map<K, V> argument");
                }
                Some(args[0].ty.clone())
            }
            "map.is_empty" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Map(_, _)) {
                    bail!("map.is_empty expects one map<K, V> argument");
                }
                Some(Type::Bool)
            }
            "map.len" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::Map(_, _)) {
                    bail!("map.len expects one map<K, V> argument");
                }
                Some(Type::I64)
            }
            "ordered_map.new" => {
                if !args.is_empty() {
                    bail!("ordered_map.new expects no arguments");
                }
                effects.insert("alloc".to_string());
                Some(Type::OrderedMap(
                    Box::new(Type::Unknown),
                    Box::new(Type::Unknown),
                ))
            }
            "ordered_map.put" => {
                if args.len() != 3 {
                    bail!("ordered_map.put expects (ordered_map<K, V>, K, V)");
                }

                let Type::OrderedMap(key_ty, value_ty) = &args[0].ty else {
                    bail!("ordered_map.put first argument must be ordered_map<K, V>");
                };

                let resolved_key_ty = if is_unknown_type(key_ty.as_ref()) {
                    args[1].ty.clone()
                } else if is_assignable_type(key_ty.as_ref(), &args[1].ty) {
                    key_ty.as_ref().clone()
                } else {
                    bail!(
                        "ordered_map.put key type mismatch: ordered_map<{:?}, _> cannot accept {:?}",
                        key_ty,
                        args[1].ty
                    );
                };

                if !is_unknown_type(&resolved_key_ty)
                    && !is_orderable_map_key_type(&resolved_key_ty)
                {
                    bail!(
                        "ordered_map<K, V> key type {:?} is not currently orderable",
                        resolved_key_ty
                    );
                }

                let resolved_value_ty = if is_unknown_type(value_ty.as_ref()) {
                    args[2].ty.clone()
                } else if is_assignable_type(value_ty.as_ref(), &args[2].ty) {
                    value_ty.as_ref().clone()
                } else {
                    bail!(
                        "ordered_map.put value type mismatch: ordered_map<_, {:?}> cannot accept {:?}",
                        value_ty,
                        args[2].ty
                    );
                };

                effects.insert("alloc".to_string());
                Some(Type::OrderedMap(
                    Box::new(resolved_key_ty),
                    Box::new(resolved_value_ty),
                ))
            }
            "ordered_map.get" => {
                if args.len() != 2 {
                    bail!("ordered_map.get expects (ordered_map<K, V>, K)");
                }

                let Type::OrderedMap(key_ty, value_ty) = &args[0].ty else {
                    bail!("ordered_map.get first argument must be ordered_map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref()) && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "ordered_map.get key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(value_ty.as_ref().clone())
            }
            "ordered_map.contains" => {
                if args.len() != 2 {
                    bail!("ordered_map.contains expects (ordered_map<K, V>, K)");
                }

                let Type::OrderedMap(key_ty, _) = &args[0].ty else {
                    bail!("ordered_map.contains first argument must be ordered_map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref()) && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "ordered_map.contains key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(Type::Bool)
            }
            "ordered_map.remove" => {
                if args.len() != 2 {
                    bail!("ordered_map.remove expects (ordered_map<K, V>, K)");
                }

                let Type::OrderedMap(key_ty, value_ty) = &args[0].ty else {
                    bail!("ordered_map.remove first argument must be ordered_map<K, V>");
                };

                if !is_unknown_type(key_ty.as_ref())
                    && !is_assignable_type(key_ty.as_ref(), &args[1].ty)
                {
                    bail!(
                        "ordered_map.remove key type mismatch: expected {:?}, got {:?}",
                        key_ty,
                        args[1].ty
                    );
                }

                Some(Type::OrderedMap(key_ty.clone(), value_ty.clone()))
            }
            "ordered_map.clear" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::OrderedMap(_, _)) {
                    bail!("ordered_map.clear expects one ordered_map<K, V> argument");
                }
                Some(args[0].ty.clone())
            }
            "ordered_map.is_empty" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::OrderedMap(_, _)) {
                    bail!("ordered_map.is_empty expects one ordered_map<K, V> argument");
                }
                Some(Type::Bool)
            }
            "ordered_map.len" => {
                if args.len() != 1 || !matches!(args[0].ty, Type::OrderedMap(_, _)) {
                    bail!("ordered_map.len expects one ordered_map<K, V> argument");
                }
                Some(Type::I64)
            }
            _ => None,
        };

        Ok(resolved.map(|ty| (ty, effects)))
    }

    fn expect_scalar(&self, value: LoweredValue) -> Result<TypedOperand> {
        match value {
            LoweredValue::Scalar(value) => Ok(value),
            LoweredValue::Range { .. } => Err(anyhow!("expected scalar expression, found range")),
            LoweredValue::Void => Err(anyhow!("expected scalar expression, found void")),
        }
    }

    fn emit(&mut self, block: BlockId, instruction: MirInstruction) -> Result<()> {
        let Some(block) = self.blocks.get_mut(block) else {
            return Err(anyhow!("invalid block id {}", block));
        };
        if block.terminator.is_some() {
            return Err(anyhow!("cannot emit instruction after block terminator"));
        }
        block.instructions.push(instruction);
        Ok(())
    }

    fn set_terminator(&mut self, block_id: BlockId, terminator: MirTerminator) -> Result<()> {
        let Some(block) = self.blocks.get_mut(block_id) else {
            return Err(anyhow!("invalid block id {}", block_id));
        };
        if block.terminator.is_some() {
            return Err(anyhow!("block '{}' already has terminator", block.label));
        }
        block.terminator = Some(terminator);
        Ok(())
    }

    fn new_block(&mut self, label_prefix: &str) -> BlockId {
        let id = self.blocks.len();
        self.blocks.push(MirBlockBuilder {
            label: format!("{label_prefix}.{id}"),
            instructions: Vec::new(),
            terminator: None,
        });
        id
    }

    fn new_temp(&mut self, ty: Type) -> String {
        let name = format!("__t{}", self.next_temp);
        self.next_temp += 1;
        self.local_types.insert(name.clone(), ty);
        name
    }

    fn define_local_with_exact_name(&mut self, source_name: &str, ty: Type) -> Result<String> {
        if self.local_types.contains_key(source_name) {
            return self.define_local(source_name, ty);
        }

        self.local_types.insert(source_name.to_string(), ty);
        self.scopes
            .last_mut()
            .ok_or_else(|| anyhow!("missing scope while defining local"))?
            .insert(source_name.to_string(), source_name.to_string());
        Ok(source_name.to_string())
    }

    fn define_local(&mut self, source_name: &str, ty: Type) -> Result<String> {
        let mut candidate = source_name.to_string();
        if self.local_types.contains_key(&candidate) {
            candidate = format!("{source_name}#{}", self.next_local);
            self.next_local += 1;
        }

        self.local_types.insert(candidate.clone(), ty);
        self.scopes
            .last_mut()
            .ok_or_else(|| anyhow!("missing scope while defining local"))?
            .insert(source_name.to_string(), candidate.clone());
        Ok(candidate)
    }

    fn define_hidden_local(&mut self, prefix: &str, ty: Type) -> Result<String> {
        let name = format!("{prefix}.{}", self.next_local);
        self.next_local += 1;
        self.local_types.insert(name.clone(), ty);
        Ok(name)
    }

    fn lookup_local(&self, source_name: &str) -> Result<(String, Type)> {
        for scope in self.scopes.iter().rev() {
            if let Some(storage) = scope.get(source_name) {
                let ty = self
                    .local_types
                    .get(storage)
                    .ok_or_else(|| anyhow!("missing type for local '{}'", storage))?
                    .clone();
                return Ok((storage.clone(), ty));
            }
        }

        Err(anyhow!("unknown local variable '{}'", source_name))
    }
}

fn lower_type_syntax_no_ctx(syntax: &TypeSyntax) -> Type {
    match syntax {
        TypeSyntax::Void => Type::Void,
        TypeSyntax::Named(name) => lower_named_type(name),
        TypeSyntax::Generic { name, args } => lower_generic_type(name, args, lower_type_syntax_no_ctx),
        TypeSyntax::Ref {
            region,
            mutable,
            inner,
        } => Type::Ref {
            region: region.clone(),
            mutable: *mutable,
            inner: Box::new(lower_type_syntax_no_ctx(inner)),
        },
        TypeSyntax::Array { inner, size } => Type::Array {
            inner: Box::new(lower_type_syntax_no_ctx(inner)),
            size: *size,
        },
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
        "ptr" => Type::Ptr(Box::new(Type::Unknown)),
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
        "ptr" if args.len() == 1 => Type::Ptr(Box::new(lower(&args[0]))),
        _ => Type::Unknown,
    }
}

fn is_integral_type(ty: &Type) -> bool {
    matches!(ty, Type::I64 | Type::I32 | Type::I8 | Type::U64 | Type::U32)
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

fn is_builtin_namespace_name(name: &str) -> bool {
    matches!(name, "io" | "str" | "vec" | "map" | "ordered_map")
}

fn is_static_builtin_member_call(module_name: &str, field: &str) -> bool {
    matches!(
        (module_name, field),
        ("io", "out")
            | ("vec", "new")
            | ("vec", "new_i64")
            | ("vec", "with_capacity")
            | ("map", "new")
            | ("map", "with_capacity")
            | ("ordered_map", "new")
    )
}

fn builtin_namespace_for_type(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Ref { inner, .. } => builtin_namespace_for_type(inner.as_ref()),
        Type::Str => Some("str"),
        Type::Vec(_) => Some("vec"),
        Type::Map(_, _) => Some("map"),
        Type::OrderedMap(_, _) => Some("ordered_map"),
        _ => None,
    }
}

fn is_assignable_type(expected: &Type, actual: &Type) -> bool {
    match (expected, actual) {
        (Type::Unknown, _) | (_, Type::Unknown) => true,
        (Type::Ptr(expected_inner), Type::Ptr(actual_inner)) => {
            is_assignable_type(expected_inner, actual_inner)
        }
        (Type::Vec(expected_inner), Type::Vec(actual_inner)) => {
            is_assignable_type(expected_inner, actual_inner)
        }
        (Type::Map(expected_k, expected_v), Type::Map(actual_k, actual_v)) => {
            is_assignable_type(expected_k, actual_k) && is_assignable_type(expected_v, actual_v)
        }
        (
            Type::OrderedMap(expected_k, expected_v),
            Type::OrderedMap(actual_k, actual_v),
        ) => {
            is_assignable_type(expected_k, actual_k)
                && is_assignable_type(expected_v, actual_v)
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
            (expected_size == actual_size || expected_size.is_none() || actual_size.is_none())
                && is_assignable_type(expected_inner, actual_inner)
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
                && is_assignable_type(expected_inner, actual_inner)
        }
        _ => expected == actual,
    }
}

fn merge_inferred_type(current: &Type, inferred: &Type) -> Type {
    match (current, inferred) {
        (Type::Unknown, other) => other.clone(),
        (other, Type::Unknown) => other.clone(),
        (Type::Ptr(current_inner), Type::Ptr(inferred_inner)) => Type::Ptr(Box::new(
            merge_inferred_type(current_inner.as_ref(), inferred_inner.as_ref()),
        )),
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

fn parse_int_literal(text: &str) -> Result<i64> {
    let numeric: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '_')
        .filter(|c| *c != '_')
        .collect();
    numeric
        .parse::<i64>()
        .map_err(|e| anyhow!("invalid integer literal '{}': {}", text, e))
}

fn parse_float_literal(text: &str) -> Result<f64> {
    let normalized: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(*c, '.' | 'e' | 'E' | '+' | '-' | '_'))
        .filter(|c| *c != '_')
        .collect();
    normalized
        .parse::<f64>()
        .map_err(|e| anyhow!("invalid float literal '{}': {}", text, e))
}

fn default_value_for_type(ty: &Type) -> MirOperand {
    match ty {
        Type::Bool => MirOperand::Const(MirConst::Bool(false)),
        Type::Str => MirOperand::Const(MirConst::Str(String::new())),
        Type::F64 => MirOperand::Const(MirConst::Float(0.0, Type::F64)),
        Type::F32 => MirOperand::Const(MirConst::Float(0.0, Type::F32)),
        Type::I64 => MirOperand::Const(MirConst::Int(0, Type::I64)),
        Type::I32 => MirOperand::Const(MirConst::Int(0, Type::I32)),
        Type::I8 => MirOperand::Const(MirConst::Int(0, Type::I8)),
        Type::U64 => MirOperand::Const(MirConst::Int(0, Type::U64)),
        Type::U32 => MirOperand::Const(MirConst::Int(0, Type::U32)),
        _ => MirOperand::Const(MirConst::Int(0, Type::I64)),
    }
}

fn int_one_for_type(ty: &Type) -> MirOperand {
    match ty {
        Type::I64 => MirOperand::Const(MirConst::Int(1, Type::I64)),
        Type::I32 => MirOperand::Const(MirConst::Int(1, Type::I32)),
        Type::I8 => MirOperand::Const(MirConst::Int(1, Type::I8)),
        Type::U64 => MirOperand::Const(MirConst::Int(1, Type::U64)),
        Type::U32 => MirOperand::Const(MirConst::Int(1, Type::U32)),
        _ => MirOperand::Const(MirConst::Int(1, Type::I64)),
    }
}

fn binary_result_type(op: BinaryOp, lhs_ty: &Type) -> Type {
    match op {
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::And
        | BinaryOp::Or => Type::Bool,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            lhs_ty.clone()
        }
        BinaryOp::Range => Type::Range(Box::new(lhs_ty.clone())),
    }
}

fn fold_unary_constant(op: UnaryOp, operand: &MirOperand) -> Option<MirConst> {
    match (op, operand) {
        (UnaryOp::Not, MirOperand::Const(MirConst::Bool(value))) => Some(MirConst::Bool(!value)),
        (UnaryOp::Neg, MirOperand::Const(MirConst::Int(value, ty))) => {
            Some(MirConst::Int(-value, ty.clone()))
        }
        (UnaryOp::Neg, MirOperand::Const(MirConst::Float(value, ty))) => {
            Some(MirConst::Float(-value, ty.clone()))
        }
        _ => None,
    }
}

fn fold_binary_constant(op: BinaryOp, lhs: &MirOperand, rhs: &MirOperand) -> Option<MirConst> {
    match (lhs, rhs) {
        (MirOperand::Const(MirConst::Int(l, ty_l)), MirOperand::Const(MirConst::Int(r, _))) => {
            let folded = match op {
                BinaryOp::Add => return Some(MirConst::Int(l + r, ty_l.clone())),
                BinaryOp::Sub => return Some(MirConst::Int(l - r, ty_l.clone())),
                BinaryOp::Mul => return Some(MirConst::Int(l * r, ty_l.clone())),
                BinaryOp::Div => return Some(MirConst::Int(l / r, ty_l.clone())),
                BinaryOp::Rem => return Some(MirConst::Int(l % r, ty_l.clone())),
                BinaryOp::Eq => MirConst::Bool(l == r),
                BinaryOp::Ne => MirConst::Bool(l != r),
                BinaryOp::Lt => MirConst::Bool(l < r),
                BinaryOp::Lte => MirConst::Bool(l <= r),
                BinaryOp::Gt => MirConst::Bool(l > r),
                BinaryOp::Gte => MirConst::Bool(l >= r),
                BinaryOp::And => MirConst::Bool((*l != 0) && (*r != 0)),
                BinaryOp::Or => MirConst::Bool((*l != 0) || (*r != 0)),
                BinaryOp::Range => return None,
            };
            Some(folded)
        }
        (
            MirOperand::Const(MirConst::Float(l, ty_l)),
            MirOperand::Const(MirConst::Float(r, _)),
        ) => {
            let folded = match op {
                BinaryOp::Add => return Some(MirConst::Float(l + r, ty_l.clone())),
                BinaryOp::Sub => return Some(MirConst::Float(l - r, ty_l.clone())),
                BinaryOp::Mul => return Some(MirConst::Float(l * r, ty_l.clone())),
                BinaryOp::Div => return Some(MirConst::Float(l / r, ty_l.clone())),
                BinaryOp::Rem => return Some(MirConst::Float(l % r, ty_l.clone())),
                BinaryOp::Eq => MirConst::Bool((l - r).abs() < f64::EPSILON),
                BinaryOp::Ne => MirConst::Bool((l - r).abs() >= f64::EPSILON),
                BinaryOp::Lt => MirConst::Bool(l < r),
                BinaryOp::Lte => MirConst::Bool(l <= r),
                BinaryOp::Gt => MirConst::Bool(l > r),
                BinaryOp::Gte => MirConst::Bool(l >= r),
                BinaryOp::And | BinaryOp::Or | BinaryOp::Range => return None,
            };
            Some(folded)
        }
        (MirOperand::Const(MirConst::Bool(l)), MirOperand::Const(MirConst::Bool(r))) => {
            match op {
                BinaryOp::And => Some(MirConst::Bool(*l && *r)),
                BinaryOp::Or => Some(MirConst::Bool(*l || *r)),
                BinaryOp::Eq => Some(MirConst::Bool(l == r)),
                BinaryOp::Ne => Some(MirConst::Bool(l != r)),
                _ => None,
            }
        }
        (MirOperand::Const(MirConst::Str(l)), MirOperand::Const(MirConst::Str(r))) => {
            match op {
                BinaryOp::Add => Some(MirConst::Str(format!("{l}{r}"))),
                BinaryOp::Eq => Some(MirConst::Bool(l == r)),
                BinaryOp::Ne => Some(MirConst::Bool(l != r)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn constant_fold_function(function: &mut MirFunction) {
    for block in &mut function.blocks {
        let mut constants = BTreeMap::<String, MirConst>::new();

        for instruction in &mut block.instructions {
            rewrite_instruction_operands(instruction, &constants);
            fold_instruction(instruction);

            if let Some(dest) = &instruction.dest {
                if let MirInstructionKind::Copy(MirOperand::Const(constant)) = &instruction.kind {
                    constants.insert(dest.clone(), constant.clone());
                } else {
                    constants.remove(dest);
                }
            }
        }

        if let MirTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } = &block.terminator
        {
            let rewritten = substitute_operand(cond, &constants);
            match rewritten {
                MirOperand::Const(MirConst::Bool(true)) => {
                    block.terminator = MirTerminator::Goto(*then_bb);
                }
                MirOperand::Const(MirConst::Bool(false)) => {
                    block.terminator = MirTerminator::Goto(*else_bb);
                }
                other => {
                    block.terminator = MirTerminator::Branch {
                        cond: other,
                        then_bb: *then_bb,
                        else_bb: *else_bb,
                    };
                }
            }
        }
    }
}

fn rewrite_instruction_operands(
    instruction: &mut MirInstruction,
    constants: &BTreeMap<String, MirConst>,
) {
    match &mut instruction.kind {
        MirInstructionKind::Copy(operand) => {
            *operand = substitute_operand(operand, constants);
        }
        MirInstructionKind::Unary { operand, .. } => {
            *operand = substitute_operand(operand, constants);
        }
        MirInstructionKind::Binary { lhs, rhs, .. } => {
            *lhs = substitute_operand(lhs, constants);
            *rhs = substitute_operand(rhs, constants);
        }
        MirInstructionKind::Call { args, .. } => {
            for arg in args {
                *arg = substitute_operand(arg, constants);
            }
        }
        MirInstructionKind::MemberLoad { base, .. } => {
            *base = substitute_operand(base, constants);
        }
        MirInstructionKind::IndexLoad { base, index } => {
            *base = substitute_operand(base, constants);
            *index = substitute_operand(index, constants);
        }
    }
}

fn fold_instruction(instruction: &mut MirInstruction) {
    match &instruction.kind {
        MirInstructionKind::Unary { op, operand } => {
            if let Some(constant) = fold_unary_constant(*op, operand) {
                instruction.kind = MirInstructionKind::Copy(MirOperand::Const(constant));
            }
        }
        MirInstructionKind::Binary { op, lhs, rhs } => {
            if let Some(constant) = fold_binary_constant(*op, lhs, rhs) {
                instruction.kind = MirInstructionKind::Copy(MirOperand::Const(constant));
            }
        }
        _ => {}
    }
}

fn substitute_operand(operand: &MirOperand, constants: &BTreeMap<String, MirConst>) -> MirOperand {
    match operand {
        MirOperand::Local(local) => constants
            .get(local)
            .cloned()
            .map(MirOperand::Const)
            .unwrap_or_else(|| MirOperand::Local(local.clone())),
        MirOperand::Const(constant) => MirOperand::Const(constant.clone()),
    }
}

fn eliminate_dead_branches(function: &mut MirFunction) {
    if function.blocks.is_empty() {
        return;
    }

    let mut reachable = BTreeSet::new();
    collect_reachable(function.entry, &function.blocks, &mut reachable);

    if reachable.len() == function.blocks.len() {
        return;
    }

    let mut remap = BTreeMap::new();
    let mut new_blocks = Vec::new();

    for old_block in &function.blocks {
        if reachable.contains(&old_block.id) {
            let new_id = new_blocks.len();
            remap.insert(old_block.id, new_id);
            new_blocks.push(old_block.clone());
        }
    }

    for block in &mut new_blocks {
        block.id = *remap.get(&block.id).expect("reachable block remap exists");
        block.terminator = remap_terminator(&block.terminator, &remap);
    }

    function.entry = *remap
        .get(&function.entry)
        .expect("entry block remains reachable");
    function.blocks = new_blocks;
}

fn collect_reachable(block_id: BlockId, blocks: &[MirBlock], reachable: &mut BTreeSet<BlockId>) {
    if !reachable.insert(block_id) {
        return;
    }

    let Some(block) = blocks.get(block_id) else {
        return;
    };

    match block.terminator {
        MirTerminator::Goto(next) => collect_reachable(next, blocks, reachable),
        MirTerminator::Branch {
            then_bb, else_bb, ..
        } => {
            collect_reachable(then_bb, blocks, reachable);
            collect_reachable(else_bb, blocks, reachable);
        }
        MirTerminator::Return(_) | MirTerminator::Unreachable => {}
    }
}

fn remap_terminator(terminator: &MirTerminator, remap: &BTreeMap<BlockId, BlockId>) -> MirTerminator {
    match terminator {
        MirTerminator::Goto(target) => MirTerminator::Goto(*remap.get(target).expect("valid remap")),
        MirTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => MirTerminator::Branch {
            cond: cond.clone(),
            then_bb: *remap.get(then_bb).expect("valid remap"),
            else_bb: *remap.get(else_bb).expect("valid remap"),
        },
        MirTerminator::Return(value) => MirTerminator::Return(value.clone()),
        MirTerminator::Unreachable => MirTerminator::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use crate::{lexer, parser, sema};

    use super::*;

    #[test]
    fn lowers_to_cfg_mir_and_folds_constants() {
        let src = r#"
fn main() -> i64
    effects(none)
{
    let x = 1 + 2
    if true {
        return x
    } else {
        return 0
    }
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());
        let (ast, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());
        let (checked, sema_diags) = sema::check(&ast);
        assert!(!sema_diags.has_errors());

        let mut mir = lower(&ast, &checked).expect("mir lowering should succeed");
        optimize(&mut mir);

        let main_fn = mir.functions.get("main").expect("main function present");
        assert!(!main_fn.blocks.is_empty());
        assert!(main_fn.blocks.len() <= 3);
    }

    #[test]
    fn lowers_for_loop_over_fixed_array_param() {
        let src = r#"
fn sum(values: [i64; 4]) -> i64
    effects(none)
{
    let acc = 0
    for value in values {
        acc += value
    }
    return acc
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (ast, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (checked, sema_diags) = sema::check(&ast);
        assert!(!sema_diags.has_errors());

        let mut mir = lower(&ast, &checked).expect("mir lowering should succeed");
        optimize(&mut mir);

        let sum_fn = mir.functions.get("sum").expect("sum function present");
        let has_index_load = sum_fn
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction.kind, MirInstructionKind::IndexLoad { .. }));
        assert!(has_index_load, "expected array iteration to emit IndexLoad");
    }

    #[test]
    fn lowers_for_loop_over_vec_and_str() {
        let src = r#"
fn main() -> i32
    effects(alloc)
{
    let total = 0s
    let text = "ab"
    for ch in text {
        total += ch
    }

    let values: vec<i32> = vec.new()
    values.push(1s)
    values.push(2s)
    for value in values {
        total += value
    }

    return total
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (ast, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (checked, sema_diags) = sema::check(&ast);
        assert!(!sema_diags.has_errors());

        let mut mir = lower(&ast, &checked).expect("mir lowering should succeed");
        optimize(&mut mir);

        let main_fn = mir.functions.get("main").expect("main function present");
        let calls = main_fn
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instruction| match &instruction.kind {
                MirInstructionKind::Call { callee, .. } => Some(callee.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        let has_vec_push_stmt = main_fn
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                matches!(
                    &instruction.kind,
                    MirInstructionKind::Call { callee, .. }
                        if callee == "vec.push"
                            && instruction.dest.is_none()
                            && instruction.ty == Type::Void
                )
            });

        assert!(calls.iter().any(|callee| *callee == "str.len"));
        assert!(calls.iter().any(|callee| *callee == "vec.len"));
        assert!(
            has_vec_push_stmt,
            "expected vec.push to lower as a void statement call"
        );

        let index_load_count = main_fn
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter(|instruction| matches!(instruction.kind, MirInstructionKind::IndexLoad { .. }))
            .count();
        assert!(
            index_load_count >= 2,
            "expected both string and vec loops to emit IndexLoad operations"
        );
    }

    #[test]
    fn lowers_ptr_signature_types() {
        let src = r#"
fn passthrough(p: ptr<i64>) -> ptr<i64>
    effects(none)
{
    return p
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (ast, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (checked, sema_diags) = sema::check(&ast);
        assert!(!sema_diags.has_errors());

        let mut mir = lower(&ast, &checked).expect("mir lowering should succeed");
        optimize(&mut mir);

        let passthrough = mir
            .functions
            .get("passthrough")
            .expect("passthrough function present");
        assert!(matches!(passthrough.params[0].ty, Type::Ptr(_)));
        assert!(matches!(passthrough.return_type, Type::Ptr(_)));
    }
}
