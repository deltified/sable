use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::types::{BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::{FloatPredicate, IntPredicate};

use crate::ast::{BinaryOp, UnaryOp};
use crate::mir::{MirConst, MirFunction, MirInstruction, MirInstructionKind, MirOperand, MirProgram, MirTerminator};
use crate::sema::Type;

pub fn emit_llvm_ir(program: &MirProgram, module_name: &str) -> Result<String> {
    let context = Context::create();
    let module = context.create_module(module_name);
    let builder = context.create_builder();

    let mut codegen = Codegen {
        context: &context,
        module,
        builder,
        functions: BTreeMap::new(),
        struct_types: BTreeMap::new(),
        struct_field_indices: BTreeMap::new(),
    };

    codegen.declare_struct_types(program)?;
    codegen.declare_functions(program)?;
    codegen.define_functions(program)?;

    Ok(codegen.module.print_to_string().to_string())
}

struct Codegen<'ctx> {
    context: &'ctx Context,
    module: LlvmModule<'ctx>,
    builder: Builder<'ctx>,
    functions: BTreeMap<String, FunctionValue<'ctx>>,
    struct_types: BTreeMap<String, StructType<'ctx>>,
    struct_field_indices: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Clone, Copy)]
struct LocalValue<'ctx> {
    ptr: PointerValue<'ctx>,
    ty: BasicTypeEnum<'ctx>,
}

struct FunctionContext<'ctx> {
    alloca_builder: Builder<'ctx>,
    locals: BTreeMap<String, LocalValue<'ctx>>,
    local_sable_types: BTreeMap<String, Type>,
    blocks: Vec<BasicBlock<'ctx>>,
}

impl<'ctx> Codegen<'ctx> {
    fn declare_struct_types(&mut self, program: &MirProgram) -> Result<()> {
        for struct_def in program.structs.values() {
            let struct_ty = self.context.opaque_struct_type(&struct_def.name);
            self.struct_types.insert(struct_def.name.clone(), struct_ty);
            self.struct_field_indices
                .insert(struct_def.name.clone(), struct_def.field_indices.clone());
        }

        for struct_def in program.structs.values() {
            let struct_ty = *self
                .struct_types
                .get(&struct_def.name)
                .ok_or_else(|| anyhow!("missing declared LLVM struct '{}': internal error", struct_def.name))?;

            let mut field_types = Vec::with_capacity(struct_def.fields.len());
            for field in &struct_def.fields {
                let field_ty = self.llvm_basic_type(&field.ty).ok_or_else(|| {
                    anyhow!(
                        "unsupported field type {:?} for struct '{}.{}'",
                        field.ty,
                        struct_def.name,
                        field.name
                    )
                })?;
                field_types.push(field_ty);
            }

            struct_ty.set_body(&field_types, false);
        }

        Ok(())
    }

    fn declare_functions(&mut self, program: &MirProgram) -> Result<()> {
        for function in program.functions.values() {
            let mut arg_types = Vec::with_capacity(function.params.len());
            for param in &function.params {
                let llvm_ty = self.llvm_basic_type(&param.ty).ok_or_else(|| {
                    anyhow!(
                        "unsupported MIR parameter type {:?} in function {}",
                        param.ty,
                        function.name
                    )
                })?;
                arg_types.push(llvm_ty.into());
            }

            let value = match self.llvm_basic_type(&function.return_type) {
                Some(ret_ty) => {
                    let fn_ty = ret_ty.fn_type(&arg_types, false);
                    self.module.add_function(&function.name, fn_ty, None)
                }
                None => {
                    if function.return_type != Type::Void {
                        bail!(
                            "unsupported MIR return type {:?} in function {}",
                            function.return_type,
                            function.name
                        );
                    }
                    let fn_ty = self.context.void_type().fn_type(&arg_types, false);
                    self.module.add_function(&function.name, fn_ty, None)
                }
            };

            self.functions.insert(function.name.clone(), value);
        }

        Ok(())
    }

    fn define_functions(&mut self, program: &MirProgram) -> Result<()> {
        for function in program.functions.values() {
            if function.blocks.is_empty() {
                continue;
            }
            self.define_single_function(function)?;
        }
        Ok(())
    }

    fn define_single_function(&mut self, function: &MirFunction) -> Result<()> {
        let llvm_function = *self
            .functions
            .get(&function.name)
            .ok_or_else(|| anyhow!("missing function declaration for {}", function.name))?;

        let alloca_block = self
            .context
            .append_basic_block(llvm_function, "entry.alloca");
        let mir_blocks = function
            .blocks
            .iter()
            .map(|block| self.context.append_basic_block(llvm_function, &block.label))
            .collect::<Vec<_>>();

        let mut function_ctx = FunctionContext {
            alloca_builder: self.context.create_builder(),
            locals: BTreeMap::new(),
            local_sable_types: BTreeMap::new(),
            blocks: mir_blocks,
        };

        function_ctx.alloca_builder.position_at_end(alloca_block);
        self.builder.position_at_end(alloca_block);

        for (index, param) in function.params.iter().enumerate() {
            let value = llvm_function
                .get_nth_param(index as u32)
                .ok_or_else(|| anyhow!("missing LLVM parameter {} for {}", index, function.name))?;
            self.store_to_local(
                &mut function_ctx,
                &param.name,
                &param.ty,
                value,
            )?;
        }

        for block in &function.blocks {
            let llvm_block = function_ctx.blocks[block.id];
            self.builder.position_at_end(llvm_block);

            for instruction in &block.instructions {
                self.emit_instruction(&mut function_ctx, instruction)?;
            }

            if self
                .builder
                .get_insert_block()
                .and_then(|bb| bb.get_terminator())
                .is_none()
            {
                self.emit_terminator(&mut function_ctx, &block.terminator)?;
            }
        }

        self.builder.position_at_end(alloca_block);
        if self
            .builder
            .get_insert_block()
            .and_then(|bb| bb.get_terminator())
            .is_none()
        {
            self.builder
                .build_unconditional_branch(function_ctx.blocks[function.entry])
                .map_err(|e| anyhow!("failed to branch to MIR entry block: {e}"))?;
        }

        Ok(())
    }

    fn emit_instruction(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        instruction: &MirInstruction,
    ) -> Result<()> {
        match &instruction.kind {
            MirInstructionKind::Copy(operand) => {
                if let Some(dest) = &instruction.dest {
                    let value = self.lower_operand(function_ctx, operand)?;
                    self.store_to_local(function_ctx, dest, &instruction.ty, value)?;
                }
            }
            MirInstructionKind::Unary { op, operand } => {
                let value = self.lower_operand(function_ctx, operand)?;
                let computed = self.emit_unary(*op, value)?;
                if let Some(dest) = &instruction.dest {
                    self.store_to_local(function_ctx, dest, &instruction.ty, computed)?;
                }
            }
            MirInstructionKind::Binary { op, lhs, rhs } => {
                let lhs = self.lower_operand(function_ctx, lhs)?;
                let rhs = self.lower_operand(function_ctx, rhs)?;
                let computed = self.emit_binary(*op, lhs, rhs)?;
                if let Some(dest) = &instruction.dest {
                    self.store_to_local(function_ctx, dest, &instruction.ty, computed)?;
                }
            }
            MirInstructionKind::Call { callee, args } => {
                let call_result = self.emit_call(function_ctx, callee, args, &instruction.ty)?;
                if let (Some(dest), Some(value)) = (&instruction.dest, call_result) {
                    self.store_to_local(function_ctx, dest, &instruction.ty, value)?;
                }
            }
            MirInstructionKind::MemberLoad { base, field } => {
                let loaded = self.emit_member_load(function_ctx, base, field, &instruction.ty)?;
                if let Some(dest) = &instruction.dest {
                    self.store_to_local(function_ctx, dest, &instruction.ty, loaded)?;
                }
            }
            MirInstructionKind::IndexLoad { base, index } => {
                let loaded = self.emit_index_load(function_ctx, base, index, &instruction.ty)?;
                if let Some(dest) = &instruction.dest {
                    self.store_to_local(function_ctx, dest, &instruction.ty, loaded)?;
                }
            }
        }

        Ok(())
    }

    fn emit_member_load(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        base: &MirOperand,
        field: &str,
        result_ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>> {
        let element_ty = self
            .llvm_basic_type(result_ty)
            .ok_or_else(|| anyhow!("unsupported member-load result type {:?}", result_ty))?;

        let (struct_ty, base_ptr) = match base {
            MirOperand::Local(name) => {
                let local = function_ctx
                    .locals
                    .get(name)
                    .ok_or_else(|| anyhow!("member base local '{}' not found", name))?;

                let struct_ty = match local.ty {
                    BasicTypeEnum::StructType(struct_ty) => struct_ty,
                    _ => {
                        bail!(
                            "member load base '{}' is not a struct local (got {:?})",
                            name,
                            local.ty
                        )
                    }
                };

                (struct_ty, local.ptr)
            }
            _ => bail!("member load currently requires a local variable base"),
        };

        let field_index = self.lookup_struct_field_index(struct_ty, field)?;
        let field_ptr = self
            .builder
            .build_struct_gep(struct_ty, base_ptr, field_index, "member.ptr")
            .map_err(|e| anyhow!("failed to build struct member GEP: {e}"))?;

        self.builder
            .build_load(element_ty, field_ptr, "member.load")
            .map_err(|e| anyhow!("failed to load struct member '{}': {e}", field))
    }

    fn emit_index_load(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        base: &MirOperand,
        index: &MirOperand,
        result_ty: &Type,
    ) -> Result<BasicValueEnum<'ctx>> {
        let element_ty = self
            .llvm_basic_type(result_ty)
            .ok_or_else(|| anyhow!("unsupported index-load result type {:?}", result_ty))?;

        let index_value = self.lower_operand(function_ctx, index)?;
        let index_int = match index_value {
            BasicValueEnum::IntValue(int) => int,
            _ => bail!("index operand must lower to an integer value"),
        };

        let MirOperand::Local(name) = base else {
            bail!("index load currently requires a local variable base")
        };

        let local = *function_ctx
            .locals
            .get(name)
            .ok_or_else(|| anyhow!("index base local '{}' not found", name))?;
        let base_sable_ty = function_ctx
            .local_sable_types
            .get(name)
            .ok_or_else(|| anyhow!("index base local '{}' is missing Sable type metadata", name))?;

        match base_sable_ty {
            Type::Array {
                inner: _,
                size: Some(_),
            } => {
                let array_ty = match local.ty {
                    BasicTypeEnum::ArrayType(array_ty) => array_ty,
                    _ => {
                        bail!(
                            "index load base '{}' is not an array local (got {:?})",
                            name,
                            local.ty
                        )
                    }
                };

                let index_i32 = self.to_i32_index(index_int)?;
                let zero = self.context.i32_type().const_zero();
                let array_ptr = unsafe {
                    self.builder
                        .build_gep(array_ty, local.ptr, &[zero, index_i32], "idx.ptr")
                        .map_err(|e| anyhow!("failed to build array index GEP: {e}"))?
                };

                self.builder
                    .build_load(element_ty, array_ptr, "idx.load")
                    .map_err(|e| anyhow!("failed to load indexed element: {e}"))
            }
            Type::Str | Type::Vec(_) => {
                let base_value = self
                    .builder
                    .build_load(local.ty, local.ptr, "idx.base")
                    .map_err(|e| anyhow!("failed to load dynamic index base '{}': {e}", name))?;
                let base_value_ty = base_value.get_type();
                let index_value_ty = index_int.get_type().as_basic_type_enum();

                let symbol = match base_sable_ty {
                    Type::Str => "str_index",
                    Type::Vec(_) => "vec_get",
                    _ => unreachable!(),
                };
                let runtime_fn = self.ensure_runtime_builtin(
                    symbol,
                    Some(element_ty),
                    &[base_value_ty, index_value_ty],
                );

                let call_site = self
                    .builder
                    .build_call(
                        runtime_fn,
                        &[base_value.into(), index_int.into()],
                        "idx.runtime",
                    )
                    .map_err(|e| anyhow!("failed to emit dynamic index load call: {e}"))?;

                call_site
                    .try_as_basic_value()
                    .basic()
                    .ok_or_else(|| anyhow!("dynamic index builtin did not return a value"))
            }
            _ => bail!(
                "index load base '{}' has unsupported type {:?}",
                name,
                base_sable_ty
            ),
        }
    }

    fn to_i32_index(&self, value: IntValue<'ctx>) -> Result<IntValue<'ctx>> {
        let i32_ty = self.context.i32_type();
        let bits = value.get_type().get_bit_width();

        if bits == 32 {
            Ok(value)
        } else if bits < 32 {
            self.builder
                .build_int_s_extend(value, i32_ty, "idx.sext")
                .map_err(|e| anyhow!("failed to extend index to i32: {e}"))
        } else {
            self.builder
                .build_int_truncate(value, i32_ty, "idx.trunc")
                .map_err(|e| anyhow!("failed to truncate index to i32: {e}"))
        }
    }

        fn lookup_struct_field_index(&self, struct_ty: StructType<'ctx>, field: &str) -> Result<u32> {
            let Some((name, _)) = self
                .struct_types
                .iter()
                .find(|(_, known_ty)| **known_ty == struct_ty)
            else {
                bail!("unknown LLVM struct encountered during member load")
            };

            let Some(indices) = self.struct_field_indices.get(name) else {
                bail!("missing field index map for struct '{}'", name)
            };

            let Some(index) = indices.get(field) else {
                bail!("struct '{}' has no field '{}'", name, field)
            };

            u32::try_from(*index).map_err(|_| anyhow!("field index overflow for '{}.{}'", name, field))
        }

    fn emit_call(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        callee: &str,
        args: &[MirOperand],
        result_ty: &Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>> {
        if callee == "io.out" {
            self.emit_io_out_builtin(function_ctx, args)?;
            return Ok(None);
        }

        if callee.contains('.') {
            return self.emit_runtime_builtin_call(function_ctx, callee, args, result_ty);
        }

        let function = *self
            .functions
            .get(callee)
            .ok_or_else(|| anyhow!("unknown callee '{}' in MIR call", callee))?;

        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(BasicMetadataValueEnum::from(self.lower_operand(function_ctx, arg)?));
        }

        let call_site = self
            .builder
            .build_call(function, &lowered_args, "calltmp")
            .map_err(|e| anyhow!("failed to emit call '{}': {e}", callee))?;

        if function.get_type().get_return_type().is_some() {
            let value = call_site
                .try_as_basic_value()
                .basic()
                        .ok_or_else(|| anyhow!("expected value from call to '{}'", callee))?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn emit_io_out_builtin(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        args: &[MirOperand],
    ) -> Result<()> {
        if args.len() != 1 {
            bail!("io.out currently expects exactly one argument");
        }

        let arg_sable_ty = match &args[0] {
            MirOperand::Local(name) => function_ctx.local_sable_types.get(name).cloned(),
            MirOperand::Const(MirConst::Str(_)) => Some(Type::Str),
            MirOperand::Const(MirConst::Bool(_)) => Some(Type::Bool),
            MirOperand::Const(MirConst::Int(_, ty)) => Some(ty.clone()),
            MirOperand::Const(MirConst::Float(_, ty)) => Some(ty.clone()),
        };

        if matches!(arg_sable_ty, Some(Type::Str)) {
            let raw = self.lower_operand(function_ctx, &args[0])?;
            let str_fn = self.ensure_runtime_builtin("io_out_str", None, &[raw.get_type()]);
            self.builder
                .build_call(str_fn, &[raw.into()], "io.out.str")
                .map_err(|e| anyhow!("failed to emit io.out(str): {e}"))?;
            return Ok(());
        }

        if matches!(arg_sable_ty, Some(Type::Bool)) {
            let raw = self.lower_operand(function_ctx, &args[0])?;
            let bool_value = self.to_bool(raw)?;
            let bool_fn = self.ensure_runtime_builtin(
                "io_out_bool",
                None,
                &[bool_value.get_type().as_basic_type_enum()],
            );
            self.builder
                .build_call(bool_fn, &[bool_value.into()], "io.out.bool")
                .map_err(|e| anyhow!("failed to emit io.out(bool): {e}"))?;
            return Ok(());
        }

        let raw = self.lower_operand(function_ctx, &args[0])?;
        let i64_value = match raw {
            BasicValueEnum::IntValue(int) => {
                let bits = int.get_type().get_bit_width();
                if bits == 64 {
                    int
                } else if bits < 64 {
                    self.builder
                        .build_int_s_extend(int, self.context.i64_type(), "io.sext")
                        .map_err(|e| anyhow!("io.out extend failed: {e}"))?
                } else {
                    self.builder
                        .build_int_truncate(int, self.context.i64_type(), "io.trunc")
                        .map_err(|e| anyhow!("io.out truncate failed: {e}"))?
                }
            }
            BasicValueEnum::FloatValue(float) => self
                .builder
                .build_float_to_signed_int(float, self.context.i64_type(), "io.ftoi")
                .map_err(|e| anyhow!("io.out float conversion failed: {e}"))?,
            _ => bail!("io.out only supports numeric arguments right now"),
        };

        let io_fn = if let Some(existing) = self.module.get_function("__sable_io_out_i64") {
            existing
        } else {
            let fn_ty = self
                .context
                .void_type()
                .fn_type(&[self.context.i64_type().into()], false);
            self.module.add_function("__sable_io_out_i64", fn_ty, None)
        };

        self.builder
            .build_call(io_fn, &[i64_value.into()], "io.out")
            .map_err(|e| anyhow!("failed to emit io.out call: {e}"))?;

        Ok(())
    }

    fn emit_runtime_builtin_call(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        callee: &str,
        args: &[MirOperand],
        result_ty: &Type,
    ) -> Result<Option<BasicValueEnum<'ctx>>> {
        let mut lowered_args = Vec::with_capacity(args.len());
        let mut arg_types = Vec::with_capacity(args.len());

        for arg in args {
            let value = self.lower_operand(function_ctx, arg)?;
            arg_types.push(value.get_type());
            lowered_args.push(value);
        }

        let return_ty = if *result_ty == Type::Void {
            None
        } else {
            Some(self.llvm_basic_type(result_ty).ok_or_else(|| {
                anyhow!(
                    "unsupported runtime builtin return type {:?} for '{}'",
                    result_ty,
                    callee
                )
            })?)
        };

        let runtime_fn = self.ensure_runtime_builtin(
            &callee.replace('.', "_"),
            return_ty,
            &arg_types,
        );

        let call_args = lowered_args
            .iter()
            .map(|value| BasicMetadataValueEnum::from(*value))
            .collect::<Vec<_>>();

        let call_site = self
            .builder
            .build_call(runtime_fn, &call_args, "rt.call")
            .map_err(|e| anyhow!("failed to emit runtime builtin call '{}': {e}", callee))?;

        if return_ty.is_some() {
            call_site
                .try_as_basic_value()
                .basic()
                .map(Some)
                .ok_or_else(|| anyhow!("runtime builtin '{}' did not return a value", callee))
        } else {
            Ok(None)
        }
    }

    fn ensure_runtime_builtin(
        &mut self,
        name: &str,
        return_ty: Option<BasicTypeEnum<'ctx>>,
        arg_tys: &[BasicTypeEnum<'ctx>],
    ) -> FunctionValue<'ctx> {
        let symbol = self.runtime_builtin_symbol(name, return_ty, arg_tys);
        if let Some(existing) = self.module.get_function(&symbol) {
            return existing;
        }

        let args = arg_tys
            .iter()
            .map(|ty| (*ty).into())
            .collect::<Vec<_>>();
        let fn_ty = match return_ty {
            Some(ret) => ret.fn_type(&args, false),
            None => self.context.void_type().fn_type(&args, false),
        };

        self.module.add_function(&symbol, fn_ty, None)
    }

    fn runtime_builtin_symbol(
        &self,
        name: &str,
        return_ty: Option<BasicTypeEnum<'ctx>>,
        arg_tys: &[BasicTypeEnum<'ctx>],
    ) -> String {
        let args_sig = if arg_tys.is_empty() {
            "none".to_string()
        } else {
            arg_tys
                .iter()
                .map(|ty| self.mangle_runtime_type(*ty))
                .collect::<Vec<_>>()
                .join("_")
        };
        let ret_sig = return_ty
            .map(|ty| self.mangle_runtime_type(ty))
            .unwrap_or_else(|| "void".to_string());

        format!("__sable_rt_{name}__{args_sig}__{ret_sig}")
    }

    fn mangle_runtime_type(&self, ty: BasicTypeEnum<'ctx>) -> String {
        match ty {
            BasicTypeEnum::IntType(int_ty) => format!("i{}", int_ty.get_bit_width()),
            BasicTypeEnum::FloatType(float_ty) => format!("f{}", float_ty.get_bit_width()),
            BasicTypeEnum::PointerType(_) => "ptr".to_string(),
            BasicTypeEnum::ArrayType(_) => "arr".to_string(),
            BasicTypeEnum::StructType(_) => "struct".to_string(),
            BasicTypeEnum::VectorType(_) => "vec".to_string(),
            _ => "ty".to_string(),
        }
    }

    fn emit_terminator(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        terminator: &MirTerminator,
    ) -> Result<()> {
        match terminator {
            MirTerminator::Goto(target) => {
                self.builder
                    .build_unconditional_branch(function_ctx.blocks[*target])
                    .map_err(|e| anyhow!("failed to emit goto terminator: {e}"))?;
            }
            MirTerminator::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                let cond = self.lower_operand(function_ctx, cond)?;
                let cond = self.to_bool(cond)?;
                self.builder
                    .build_conditional_branch(cond, function_ctx.blocks[*then_bb], function_ctx.blocks[*else_bb])
                    .map_err(|e| anyhow!("failed to emit branch terminator: {e}"))?;
            }
            MirTerminator::Return(value) => {
                if let Some(value) = value {
                    let lowered = self.lower_operand(function_ctx, value)?;
                    self.builder
                        .build_return(Some(&lowered))
                        .map_err(|e| anyhow!("failed to emit return: {e}"))?;
                } else {
                    self.builder
                        .build_return(None)
                        .map_err(|e| anyhow!("failed to emit void return: {e}"))?;
                }
            }
            MirTerminator::Unreachable => {
                self.builder
                    .build_unreachable()
                    .map_err(|e| anyhow!("failed to emit unreachable terminator: {e}"))?;
            }
        }

        Ok(())
    }

    fn lower_operand(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        operand: &MirOperand,
    ) -> Result<BasicValueEnum<'ctx>> {
        match operand {
            MirOperand::Local(name) => {
                let local = function_ctx
                    .locals
                    .get(name)
                    .ok_or_else(|| anyhow!("use of undeclared MIR local '{}'", name))?;
                self.builder
                    .build_load(local.ty, local.ptr, name)
                    .map_err(|e| anyhow!("failed to load local '{}': {e}", name))
            }
            MirOperand::Const(constant) => self.lower_constant(constant),
        }
    }

    fn lower_constant(&self, constant: &MirConst) -> Result<BasicValueEnum<'ctx>> {
        match constant {
            MirConst::Int(value, ty) => {
                let Some(llvm_ty) = self.llvm_basic_type(ty) else {
                    bail!("unsupported integer constant type {:?}", ty);
                };
                let int_type = llvm_ty.into_int_type();
                Ok(int_type.const_int(*value as u64, true).into())
            }
            MirConst::Float(value, ty) => {
                let Some(llvm_ty) = self.llvm_basic_type(ty) else {
                    bail!("unsupported float constant type {:?}", ty);
                };
                let float_type = llvm_ty.into_float_type();
                Ok(float_type.const_float(*value).into())
            }
            MirConst::Bool(value) => Ok(self
                .context
                .bool_type()
                .const_int(u64::from(*value), false)
                .into()),
            MirConst::Str(value) => {
                let global = self
                    .builder
                    .build_global_string_ptr(value, "str.lit")
                    .map_err(|e| anyhow!("failed to lower string constant: {e}"))?;
                Ok(global.as_pointer_value().into())
            }
        }
    }

    fn store_to_local(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        name: &str,
        ty: &Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<()> {
        let local = self.ensure_local(function_ctx, name, ty)?;
        function_ctx
            .local_sable_types
            .insert(name.to_string(), ty.clone());
        self.builder
            .build_store(local.ptr, value)
            .map_err(|e| anyhow!("failed to store local '{}': {e}", name))?;
        Ok(())
    }

    fn ensure_local(
        &mut self,
        function_ctx: &mut FunctionContext<'ctx>,
        name: &str,
        ty: &Type,
    ) -> Result<LocalValue<'ctx>> {
        if let Some(local) = function_ctx.locals.get(name) {
            return Ok(*local);
        }

        let llvm_ty = self
            .llvm_basic_type(ty)
            .ok_or_else(|| anyhow!("unsupported local type {:?} for '{}'", ty, name))?;
        let ptr = function_ctx
            .alloca_builder
            .build_alloca(llvm_ty, name)
            .map_err(|e| anyhow!("failed to allocate local '{}': {e}", name))?;

        let local = LocalValue { ptr, ty: llvm_ty };
        function_ctx.locals.insert(name.to_string(), local);
        Ok(local)
    }

    fn emit_unary(&mut self, op: UnaryOp, value: BasicValueEnum<'ctx>) -> Result<BasicValueEnum<'ctx>> {
        match op {
            UnaryOp::Neg => match value {
                BasicValueEnum::IntValue(int) => self
                    .builder
                    .build_int_neg(int, "ineg")
                    .map(|v| v.into())
                    .map_err(|e| anyhow!("int neg failed: {e}")),
                BasicValueEnum::FloatValue(float) => self
                    .builder
                    .build_float_neg(float, "fneg")
                    .map(|v| v.into())
                    .map_err(|e| anyhow!("float neg failed: {e}")),
                _ => bail!("invalid unary neg operand"),
            },
            UnaryOp::Not => {
                let value = self.to_bool(value)?;
                self.builder
                    .build_not(value, "not")
                    .map(|v| v.into())
                    .map_err(|e| anyhow!("logical not failed: {e}"))
            }
        }
    }

    fn emit_binary(
        &mut self,
        op: BinaryOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>> {
        match (lhs, rhs) {
            (BasicValueEnum::IntValue(lhs), BasicValueEnum::IntValue(rhs)) => {
                let value = match op {
                    BinaryOp::Add => self
                        .builder
                        .build_int_add(lhs, rhs, "iadd")
                        .map_err(|e| anyhow!("int add failed: {e}"))?
                        .into(),
                    BinaryOp::Sub => self
                        .builder
                        .build_int_sub(lhs, rhs, "isub")
                        .map_err(|e| anyhow!("int sub failed: {e}"))?
                        .into(),
                    BinaryOp::Mul => self
                        .builder
                        .build_int_mul(lhs, rhs, "imul")
                        .map_err(|e| anyhow!("int mul failed: {e}"))?
                        .into(),
                    BinaryOp::Div => self
                        .builder
                        .build_int_signed_div(lhs, rhs, "idiv")
                        .map_err(|e| anyhow!("int div failed: {e}"))?
                        .into(),
                    BinaryOp::Rem => self
                        .builder
                        .build_int_signed_rem(lhs, rhs, "irem")
                        .map_err(|e| anyhow!("int rem failed: {e}"))?
                        .into(),
                    BinaryOp::Eq => self
                        .builder
                        .build_int_compare(IntPredicate::EQ, lhs, rhs, "ieq")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::Ne => self
                        .builder
                        .build_int_compare(IntPredicate::NE, lhs, rhs, "ine")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::Lt => self
                        .builder
                        .build_int_compare(IntPredicate::SLT, lhs, rhs, "ilt")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::Lte => self
                        .builder
                        .build_int_compare(IntPredicate::SLE, lhs, rhs, "ilte")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::Gt => self
                        .builder
                        .build_int_compare(IntPredicate::SGT, lhs, rhs, "igt")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::Gte => self
                        .builder
                        .build_int_compare(IntPredicate::SGE, lhs, rhs, "igte")
                        .map_err(|e| anyhow!("int compare failed: {e}"))?
                        .into(),
                    BinaryOp::And => self
                        .builder
                        .build_and(lhs, rhs, "iand")
                        .map_err(|e| anyhow!("int and failed: {e}"))?
                        .into(),
                    BinaryOp::Or => self
                        .builder
                        .build_or(lhs, rhs, "ior")
                        .map_err(|e| anyhow!("int or failed: {e}"))?
                        .into(),
                    BinaryOp::Range => bail!("range binary op should not reach codegen"),
                };
                Ok(value)
            }
            (BasicValueEnum::FloatValue(lhs), BasicValueEnum::FloatValue(rhs)) => {
                let value = match op {
                    BinaryOp::Add => self
                        .builder
                        .build_float_add(lhs, rhs, "fadd")
                        .map_err(|e| anyhow!("float add failed: {e}"))?
                        .into(),
                    BinaryOp::Sub => self
                        .builder
                        .build_float_sub(lhs, rhs, "fsub")
                        .map_err(|e| anyhow!("float sub failed: {e}"))?
                        .into(),
                    BinaryOp::Mul => self
                        .builder
                        .build_float_mul(lhs, rhs, "fmul")
                        .map_err(|e| anyhow!("float mul failed: {e}"))?
                        .into(),
                    BinaryOp::Div => self
                        .builder
                        .build_float_div(lhs, rhs, "fdiv")
                        .map_err(|e| anyhow!("float div failed: {e}"))?
                        .into(),
                    BinaryOp::Rem => self
                        .builder
                        .build_float_rem(lhs, rhs, "frem")
                        .map_err(|e| anyhow!("float rem failed: {e}"))?
                        .into(),
                    BinaryOp::Eq => self
                        .builder
                        .build_float_compare(FloatPredicate::OEQ, lhs, rhs, "feq")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::Ne => self
                        .builder
                        .build_float_compare(FloatPredicate::ONE, lhs, rhs, "fne")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::Lt => self
                        .builder
                        .build_float_compare(FloatPredicate::OLT, lhs, rhs, "flt")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::Lte => self
                        .builder
                        .build_float_compare(FloatPredicate::OLE, lhs, rhs, "flte")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::Gt => self
                        .builder
                        .build_float_compare(FloatPredicate::OGT, lhs, rhs, "fgt")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::Gte => self
                        .builder
                        .build_float_compare(FloatPredicate::OGE, lhs, rhs, "fgte")
                        .map_err(|e| anyhow!("float compare failed: {e}"))?
                        .into(),
                    BinaryOp::And | BinaryOp::Or | BinaryOp::Range => {
                        bail!("invalid float binary operator")
                    }
                };
                Ok(value)
            }
            _ => bail!("binary operands must both be ints or both be floats"),
        }
    }

    fn to_bool(&self, value: BasicValueEnum<'ctx>) -> Result<inkwell::values::IntValue<'ctx>> {
        match value {
            BasicValueEnum::IntValue(int) => {
                if int.get_type().get_bit_width() == 1 {
                    Ok(int)
                } else {
                    self.builder
                        .build_int_compare(IntPredicate::NE, int, int.get_type().const_zero(), "cond.int")
                        .map_err(|e| anyhow!("int to bool conversion failed: {e}"))
                }
            }
            BasicValueEnum::FloatValue(float) => self
                .builder
                .build_float_compare(
                    FloatPredicate::ONE,
                    float,
                    float.get_type().const_zero(),
                    "cond.float",
                )
                .map_err(|e| anyhow!("float to bool conversion failed: {e}")),
            _ => bail!("unsupported value type in condition"),
        }
    }

    fn llvm_basic_type(&self, ty: &Type) -> Option<BasicTypeEnum<'ctx>> {
        Some(match ty {
            Type::Bool => self.context.bool_type().into(),
            Type::I64 => self.context.i64_type().into(),
            Type::I32 => self.context.i32_type().into(),
            Type::I8 => self.context.i8_type().into(),
            Type::U64 => self.context.i64_type().into(),
            Type::U32 => self.context.i32_type().into(),
            Type::F64 => self.context.f64_type().into(),
            Type::F32 => self.context.f32_type().into(),
            Type::Str
            | Type::Vec(_)
            | Type::Map(_, _)
            | Type::OrderedMap(_, _)
            | Type::Unknown => self.context.ptr_type(AddressSpace::default()).into(),
            Type::Named(name) => self.struct_types.get(name).copied()?.as_basic_type_enum(),
            Type::Ref { .. } => self.context.ptr_type(AddressSpace::default()).into(),
            Type::Array {
                inner,
                size: Some(size),
            } => self
                .llvm_basic_type(inner.as_ref())?
                .array_type(*size as u32)
                .into(),
            _ => return None,
        })
    }
}
