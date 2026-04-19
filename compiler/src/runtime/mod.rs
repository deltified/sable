use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};

use crate::ast::{BinaryOp, UnaryOp};
use crate::mir::{MirConst, MirInstructionKind, MirOperand, MirProgram, MirTerminator};

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    VecI64(Vec<i64>),
    Array(Vec<RuntimeValue>),
    Struct {
        name: String,
        fields: Vec<RuntimeValue>,
    },
}

impl std::fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_value(self))
    }
}

pub fn run_main(program: &MirProgram) -> Result<Option<RuntimeValue>> {
    run_function(program, "main", Vec::new())
}

fn run_function(
    program: &MirProgram,
    function_name: &str,
    args: Vec<RuntimeValue>,
) -> Result<Option<RuntimeValue>> {
    let function = program
        .functions
        .get(function_name)
        .ok_or_else(|| anyhow!("entry function '{}' not found", function_name))?;

    if function.blocks.is_empty() {
        bail!("cannot execute extern function '{}'", function_name);
    }

    if args.len() != function.params.len() {
        bail!(
            "function '{}' expects {} args, got {}",
            function_name,
            function.params.len(),
            args.len()
        );
    }

    let mut locals = BTreeMap::<String, RuntimeValue>::new();
    for (param, arg) in function.params.iter().zip(args.into_iter()) {
        locals.insert(param.name.clone(), arg);
    }

    let mut current_block = function.entry;
    loop {
        let block = function
            .blocks
            .get(current_block)
            .ok_or_else(|| anyhow!("invalid block id {} in '{}'", current_block, function_name))?;

        for instruction in &block.instructions {
            let result = match &instruction.kind {
                MirInstructionKind::Copy(operand) => Some(eval_operand(&locals, operand)?),
                MirInstructionKind::Unary { op, operand } => {
                    let value = eval_operand(&locals, operand)?;
                    Some(eval_unary(*op, value)?)
                }
                MirInstructionKind::Binary { op, lhs, rhs } => {
                    let lhs = eval_operand(&locals, lhs)?;
                    let rhs = eval_operand(&locals, rhs)?;
                    Some(eval_binary(*op, lhs, rhs)?)
                }
                MirInstructionKind::Call { callee, args } => {
                    let call_args = args
                        .iter()
                        .map(|arg| eval_operand(&locals, arg))
                        .collect::<Result<Vec<_>>>()?;

                    let (handled, builtin_result) = run_builtin(callee, &call_args)?;
                    if handled {
                        builtin_result
                    } else {
                        run_function(program, callee, call_args)?
                    }
                }
                MirInstructionKind::MemberLoad { base, field } => {
                    let base_value = eval_operand(&locals, base)?;
                    Some(eval_member_load(program, base_value, field)?)
                }
                MirInstructionKind::IndexLoad { base, index } => {
                    let base_value = eval_operand(&locals, base)?;
                    let index_value = eval_operand(&locals, index)?;
                    Some(eval_index_load(base_value, index_value)?)
                }
            };

            if let (Some(dest), Some(value)) = (&instruction.dest, result) {
                locals.insert(dest.clone(), value);
            }
        }

        match &block.terminator {
            MirTerminator::Goto(target) => current_block = *target,
            MirTerminator::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                let cond_value = eval_operand(&locals, cond)?;
                current_block = if value_as_bool(&cond_value)? {
                    *then_bb
                } else {
                    *else_bb
                };
            }
            MirTerminator::Return(value) => {
                if let Some(value) = value {
                    return Ok(Some(eval_operand(&locals, value)?));
                }
                return Ok(None);
            }
            MirTerminator::Unreachable => {
                bail!("reached unreachable block while executing '{}'", function_name)
            }
        }
    }
}

fn eval_operand(locals: &BTreeMap<String, RuntimeValue>, operand: &MirOperand) -> Result<RuntimeValue> {
    match operand {
        MirOperand::Local(name) => locals
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("use of uninitialized local '{}'", name)),
        MirOperand::Const(constant) => Ok(eval_const(constant)),
    }
}

fn eval_const(constant: &MirConst) -> RuntimeValue {
    match constant {
        MirConst::Int(value, _) => RuntimeValue::Int(*value),
        MirConst::Float(value, _) => RuntimeValue::Float(*value),
        MirConst::Bool(value) => RuntimeValue::Bool(*value),
        MirConst::Str(value) => RuntimeValue::Str(value.clone()),
    }
}

fn eval_unary(op: UnaryOp, value: RuntimeValue) -> Result<RuntimeValue> {
    match (op, value) {
        (UnaryOp::Neg, RuntimeValue::Int(value)) => Ok(RuntimeValue::Int(-value)),
        (UnaryOp::Neg, RuntimeValue::Float(value)) => Ok(RuntimeValue::Float(-value)),
        (UnaryOp::Not, RuntimeValue::Bool(value)) => Ok(RuntimeValue::Bool(!value)),
        (UnaryOp::Not, other) => Ok(RuntimeValue::Bool(!value_as_bool(&other)?)),
        (_, other) => bail!("invalid unary operation for value {:?}", other),
    }
}

fn eval_binary(op: BinaryOp, lhs: RuntimeValue, rhs: RuntimeValue) -> Result<RuntimeValue> {
    match (op, lhs, rhs) {
        (BinaryOp::Add, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Int(lhs + rhs))
        }
        (BinaryOp::Sub, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Int(lhs - rhs))
        }
        (BinaryOp::Mul, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Int(lhs * rhs))
        }
        (BinaryOp::Div, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Int(lhs / rhs))
        }
        (BinaryOp::Rem, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Int(lhs % rhs))
        }
        (BinaryOp::Eq, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs == rhs))
        }
        (BinaryOp::Ne, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs != rhs))
        }
        (BinaryOp::Lt, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs < rhs))
        }
        (BinaryOp::Lte, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs <= rhs))
        }
        (BinaryOp::Gt, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs > rhs))
        }
        (BinaryOp::Gte, RuntimeValue::Int(lhs), RuntimeValue::Int(rhs)) => {
            Ok(RuntimeValue::Bool(lhs >= rhs))
        }
        (BinaryOp::And, RuntimeValue::Bool(lhs), RuntimeValue::Bool(rhs)) => {
            Ok(RuntimeValue::Bool(lhs && rhs))
        }
        (BinaryOp::Or, RuntimeValue::Bool(lhs), RuntimeValue::Bool(rhs)) => {
            Ok(RuntimeValue::Bool(lhs || rhs))
        }
        (BinaryOp::Add, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Float(lhs + rhs))
        }
        (BinaryOp::Sub, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Float(lhs - rhs))
        }
        (BinaryOp::Mul, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Float(lhs * rhs))
        }
        (BinaryOp::Div, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Float(lhs / rhs))
        }
        (BinaryOp::Rem, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Float(lhs % rhs))
        }
        (BinaryOp::Eq, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool((lhs - rhs).abs() < f64::EPSILON))
        }
        (BinaryOp::Ne, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool((lhs - rhs).abs() >= f64::EPSILON))
        }
        (BinaryOp::Lt, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool(lhs < rhs))
        }
        (BinaryOp::Lte, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool(lhs <= rhs))
        }
        (BinaryOp::Gt, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool(lhs > rhs))
        }
        (BinaryOp::Gte, RuntimeValue::Float(lhs), RuntimeValue::Float(rhs)) => {
            Ok(RuntimeValue::Bool(lhs >= rhs))
        }
        (BinaryOp::Add, RuntimeValue::Str(lhs), RuntimeValue::Str(rhs)) => {
            Ok(RuntimeValue::Str(format!("{lhs}{rhs}")))
        }
        (BinaryOp::Eq, RuntimeValue::Str(lhs), RuntimeValue::Str(rhs)) => {
            Ok(RuntimeValue::Bool(lhs == rhs))
        }
        (BinaryOp::Ne, RuntimeValue::Str(lhs), RuntimeValue::Str(rhs)) => {
            Ok(RuntimeValue::Bool(lhs != rhs))
        }
        (BinaryOp::Eq, lhs, rhs) => Ok(RuntimeValue::Bool(lhs == rhs)),
        (BinaryOp::Ne, lhs, rhs) => Ok(RuntimeValue::Bool(lhs != rhs)),
        (BinaryOp::Range, _, _) => bail!("range operands should not be evaluated directly"),
        (op, lhs, rhs) => bail!("unsupported binary op {:?} for values {:?} and {:?}", op, lhs, rhs),
    }
}

fn eval_member_load(program: &MirProgram, base: RuntimeValue, field: &str) -> Result<RuntimeValue> {
    let RuntimeValue::Struct { name, fields } = base else {
        bail!("member access requires struct value")
    };

    let struct_info = program
        .structs
        .get(&name)
        .ok_or_else(|| anyhow!("unknown struct '{}' at runtime", name))?;
    let index = struct_info
        .field_indices
        .get(field)
        .ok_or_else(|| anyhow!("struct '{}' has no field '{}'", name, field))?;

    fields
        .get(*index)
        .cloned()
        .ok_or_else(|| anyhow!("field index out of bounds for '{}.{}'", name, field))
}

fn eval_index_load(base: RuntimeValue, index: RuntimeValue) -> Result<RuntimeValue> {
    let index = value_as_index(&index)?;

    match base {
        RuntimeValue::Array(values) => values
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("array index {} out of bounds", index)),
        RuntimeValue::Str(text) => text
            .chars()
            .nth(index)
            .map(|ch| RuntimeValue::Int(ch as i64))
            .ok_or_else(|| anyhow!("string index {} out of bounds", index)),
        RuntimeValue::VecI64(values) => values
            .get(index)
            .copied()
            .map(RuntimeValue::Int)
            .ok_or_else(|| anyhow!("vector index {} out of bounds", index)),
        other => bail!("index load requires array/vector/string base, got {:?}", other),
    }
}

fn value_as_bool(value: &RuntimeValue) -> Result<bool> {
    match value {
        RuntimeValue::Bool(value) => Ok(*value),
        RuntimeValue::Int(value) => Ok(*value != 0),
        RuntimeValue::Float(value) => Ok(*value != 0.0),
        _ => bail!("cannot convert {:?} to bool", value),
    }
}

fn value_as_index(value: &RuntimeValue) -> Result<usize> {
    match value {
        RuntimeValue::Int(value) if *value >= 0 => Ok(*value as usize),
        RuntimeValue::Int(value) => bail!("negative index {} is not allowed", value),
        _ => bail!("index value must be an integer"),
    }
}

fn run_builtin(callee: &str, args: &[RuntimeValue]) -> Result<(bool, Option<RuntimeValue>)> {
    match callee {
        "io.out" => {
            if args.len() != 1 {
                bail!("io.out expects exactly one argument")
            }
            println!("{}", format_value(&args[0]));
            Ok((true, None))
        }
        "str.concat" => {
            if args.len() != 2 {
                bail!("str.concat expects exactly two arguments")
            }
            let RuntimeValue::Str(lhs) = &args[0] else {
                bail!("str.concat first argument must be str")
            };
            let RuntimeValue::Str(rhs) = &args[1] else {
                bail!("str.concat second argument must be str")
            };
            Ok((true, Some(RuntimeValue::Str(format!("{lhs}{rhs}")))))
        }
        "str.len" => {
            if args.len() != 1 {
                bail!("str.len expects exactly one argument")
            }
            let RuntimeValue::Str(value) = &args[0] else {
                bail!("str.len argument must be str")
            };
            Ok((true, Some(RuntimeValue::Int(value.chars().count() as i64))))
        }
        "vec.new_i64" => {
            if !args.is_empty() {
                bail!("vec.new_i64 expects no arguments")
            }
            Ok((true, Some(RuntimeValue::VecI64(Vec::new()))))
        }
        "vec.push" => {
            if args.len() != 2 {
                bail!("vec.push expects exactly two arguments")
            }
            let RuntimeValue::VecI64(values) = &args[0] else {
                bail!("vec.push first argument must be vec_i64")
            };
            let RuntimeValue::Int(value) = args[1] else {
                bail!("vec.push second argument must be i64")
            };

            let mut next = values.clone();
            next.push(value);
            Ok((true, Some(RuntimeValue::VecI64(next))))
        }
        "vec.get" => {
            if args.len() != 2 {
                bail!("vec.get expects exactly two arguments")
            }
            let RuntimeValue::VecI64(values) = &args[0] else {
                bail!("vec.get first argument must be vec_i64")
            };
            let index = value_as_index(&args[1])?;
            let value = values
                .get(index)
                .copied()
                .ok_or_else(|| anyhow!("vec.get index {} out of bounds", index))?;
            Ok((true, Some(RuntimeValue::Int(value))))
        }
        "vec.len" => {
            if args.len() != 1 {
                bail!("vec.len expects exactly one argument")
            }
            let RuntimeValue::VecI64(values) = &args[0] else {
                bail!("vec.len argument must be vec_i64")
            };
            Ok((true, Some(RuntimeValue::Int(values.len() as i64))))
        }
        _ => Ok((false, None)),
    }
}

pub fn format_value(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::Void => "void".to_string(),
        RuntimeValue::Int(value) => value.to_string(),
        RuntimeValue::Float(value) => value.to_string(),
        RuntimeValue::Bool(value) => value.to_string(),
        RuntimeValue::Str(value) => value.clone(),
        RuntimeValue::VecI64(values) => {
            let inner = values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        RuntimeValue::Array(values) => {
            let inner = values
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        RuntimeValue::Struct { name, fields } => {
            let inner = fields
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}({inner})")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lexer, mir, parser, sema};

    use super::*;

    #[test]
    fn executes_strings_and_vectors() {
        let src = r#"
fn main() -> i64
    effects(io, alloc)
{
    let hello = "Hello, "
    let target = "Sable"
    let message = hello + target
    io.out(message)

    let v = vec.new_i64()
    v = vec.push(v, 7)
    v = vec.push(v, 35)

    let count = vec.len(v)
    io.out(count)

    let result = vec.get(v, 1)
    return result
}
"#;

        let (tokens, lex_diags) = lexer::lex(0, src);
        assert!(!lex_diags.has_errors());

        let (module, parse_diags) = parser::parse(tokens);
        assert!(!parse_diags.has_errors());

        let (checked, sema_diags) = sema::check(&module);
        assert!(!sema_diags.has_errors());

        let mut program = mir::lower(&module, &checked).expect("MIR lowering should succeed");
        mir::optimize(&mut program);

        let value = run_main(&program).expect("runtime should execute");
        assert_eq!(value, Some(RuntimeValue::Int(35)));
    }
}
