use std::collections::BTreeMap;

use ahash::RandomState;
use anyhow::{Result, anyhow, bail};
use hashbrown::HashMap;

use crate::ast::{BinaryOp, UnaryOp};
use crate::mir::{MirConst, MirInstructionKind, MirOperand, MirProgram, MirTerminator};

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Vec(Vec<RuntimeValue>),
    Map(HashMap<RuntimeKey, RuntimeValue, RandomState>),
    OrderedMap(BTreeMap<RuntimeKey, RuntimeValue>),
    Array(Vec<RuntimeValue>),
    Struct {
        name: String,
        fields: Vec<RuntimeValue>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeKey {
    Bool(bool),
    Int(i64),
    Str(String),
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

                    if callee.contains('.') {
                        let (handled, builtin_result) = run_builtin(callee, call_args)?;
                        if handled {
                            builtin_result
                        } else {
                            bail!("unknown builtin '{}'", callee)
                        }
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
        RuntimeValue::Vec(values) => values
            .get(index)
            .cloned()
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

fn run_builtin(callee: &str, mut args: Vec<RuntimeValue>) -> Result<(bool, Option<RuntimeValue>)> {
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

            let RuntimeValue::Str(rhs) = args.pop().expect("arg exists") else {
                bail!("str.concat second argument must be str")
            };
            let RuntimeValue::Str(lhs) = args.pop().expect("arg exists") else {
                bail!("str.concat first argument must be str")
            };

            Ok((true, Some(RuntimeValue::Str(format!("{lhs}{rhs}")))))
        }
        "str.len" => {
            if args.len() != 1 {
                bail!("str.len expects exactly one argument")
            }
            let RuntimeValue::Str(value) = args.pop().expect("arg exists") else {
                bail!("str.len argument must be str")
            };
            Ok((true, Some(RuntimeValue::Int(value.chars().count() as i64))))
        }
        "str.contains" => {
            if args.len() != 2 {
                bail!("str.contains expects exactly two arguments")
            }
            let RuntimeValue::Str(needle) = args.pop().expect("arg exists") else {
                bail!("str.contains second argument must be str")
            };
            let RuntimeValue::Str(haystack) = args.pop().expect("arg exists") else {
                bail!("str.contains first argument must be str")
            };
            Ok((true, Some(RuntimeValue::Bool(haystack.contains(&needle)))))
        }
        "str.starts_with" => {
            if args.len() != 2 {
                bail!("str.starts_with expects exactly two arguments")
            }
            let RuntimeValue::Str(prefix) = args.pop().expect("arg exists") else {
                bail!("str.starts_with second argument must be str")
            };
            let RuntimeValue::Str(text) = args.pop().expect("arg exists") else {
                bail!("str.starts_with first argument must be str")
            };
            Ok((true, Some(RuntimeValue::Bool(text.starts_with(&prefix)))))
        }
        "str.ends_with" => {
            if args.len() != 2 {
                bail!("str.ends_with expects exactly two arguments")
            }
            let RuntimeValue::Str(suffix) = args.pop().expect("arg exists") else {
                bail!("str.ends_with second argument must be str")
            };
            let RuntimeValue::Str(text) = args.pop().expect("arg exists") else {
                bail!("str.ends_with first argument must be str")
            };
            Ok((true, Some(RuntimeValue::Bool(text.ends_with(&suffix)))))
        }
        "str.find" => {
            if args.len() != 2 {
                bail!("str.find expects exactly two arguments")
            }
            let RuntimeValue::Str(needle) = args.pop().expect("arg exists") else {
                bail!("str.find second argument must be str")
            };
            let RuntimeValue::Str(text) = args.pop().expect("arg exists") else {
                bail!("str.find first argument must be str")
            };

            let index = text.find(&needle).map(|idx| idx as i64).unwrap_or(-1);
            Ok((true, Some(RuntimeValue::Int(index))))
        }
        "str.slice" => {
            if args.len() != 3 {
                bail!("str.slice expects arguments (str, start, len)")
            }

            let len = value_as_index(&args[2])?;
            let start = value_as_index(&args[1])?;
            let RuntimeValue::Str(text) = args.remove(0) else {
                bail!("str.slice first argument must be str")
            };

            let slice = text.chars().skip(start).take(len).collect::<String>();
            Ok((true, Some(RuntimeValue::Str(slice))))
        }
        "vec.new" | "vec.new_i64" => {
            if !args.is_empty() {
                bail!("{} expects no arguments", callee)
            }
            Ok((true, Some(RuntimeValue::Vec(Vec::new()))))
        }
        "vec.with_capacity" => {
            if args.len() != 1 {
                bail!("vec.with_capacity expects one argument")
            }

            let cap = value_as_index(&args[0])?;
            Ok((true, Some(RuntimeValue::Vec(Vec::with_capacity(cap)))))
        }
        "vec.push" => {
            if args.len() != 2 {
                bail!("vec.push expects exactly two arguments")
            }
            let value = args.pop().expect("arg exists");
            let RuntimeValue::Vec(mut values) = args.pop().expect("arg exists") else {
                bail!("vec.push first argument must be vec<T>")
            };

            values.push(value);
            Ok((true, Some(RuntimeValue::Vec(values))))
        }
        "vec.get" => {
            if args.len() != 2 {
                bail!("vec.get expects exactly two arguments")
            }
            let index = value_as_index(&args[1])?;
            let RuntimeValue::Vec(values) = args.remove(0) else {
                bail!("vec.get first argument must be vec<T>")
            };

            let value = values
                .get(index)
                .cloned()
                .ok_or_else(|| anyhow!("vec.get index {} out of bounds", index))?;
            Ok((true, Some(value)))
        }
        "vec.remove" => {
            if args.len() != 2 {
                bail!("vec.remove expects exactly two arguments")
            }
            let index = value_as_index(&args[1])?;
            let RuntimeValue::Vec(mut values) = args.remove(0) else {
                bail!("vec.remove first argument must be vec<T>")
            };

            if index >= values.len() {
                bail!("vec.remove index {} out of bounds", index)
            }
            values.remove(index);
            Ok((true, Some(RuntimeValue::Vec(values))))
        }
        "vec.clear" => {
            if args.len() != 1 {
                bail!("vec.clear expects exactly one argument")
            }
            let RuntimeValue::Vec(mut values) = args.pop().expect("arg exists") else {
                bail!("vec.clear argument must be vec<T>")
            };
            values.clear();
            Ok((true, Some(RuntimeValue::Vec(values))))
        }
        "vec.is_empty" => {
            if args.len() != 1 {
                bail!("vec.is_empty expects exactly one argument")
            }
            let RuntimeValue::Vec(values) = args.pop().expect("arg exists") else {
                bail!("vec.is_empty argument must be vec<T>")
            };
            Ok((true, Some(RuntimeValue::Bool(values.is_empty()))))
        }
        "vec.len" => {
            if args.len() != 1 {
                bail!("vec.len expects exactly one argument")
            }
            let RuntimeValue::Vec(values) = args.pop().expect("arg exists") else {
                bail!("vec.len argument must be vec<T>")
            };
            Ok((true, Some(RuntimeValue::Int(values.len() as i64))))
        }
        "map.new" => {
            if !args.is_empty() {
                bail!("map.new expects no arguments")
            }
            Ok((
                true,
                Some(RuntimeValue::Map(HashMap::with_hasher(RandomState::new()))),
            ))
        }
        "map.with_capacity" => {
            if args.len() != 1 {
                bail!("map.with_capacity expects one capacity argument")
            }

            let capacity = value_as_index(&args[0])?;
            Ok((
                true,
                Some(RuntimeValue::Map(HashMap::with_capacity_and_hasher(
                    capacity,
                    RandomState::new(),
                ))),
            ))
        }
        "map.put" => {
            if args.len() != 3 {
                bail!("map.put expects arguments (map<K, V>, K, V)")
            }

            let value = args.pop().expect("arg exists");
            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::Map(mut map) = args.pop().expect("arg exists") else {
                bail!("map.put first argument must be map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            map.insert(key, value);
            Ok((true, Some(RuntimeValue::Map(map))))
        }
        "map.get" => {
            if args.len() != 2 {
                bail!("map.get expects arguments (map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::Map(map) = args.pop().expect("arg exists") else {
                bail!("map.get first argument must be map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            let value = map
                .get(&key)
                .cloned()
                .ok_or_else(|| anyhow!("map.get key not found"))?;
            Ok((true, Some(value)))
        }
        "map.contains" => {
            if args.len() != 2 {
                bail!("map.contains expects arguments (map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::Map(map) = args.pop().expect("arg exists") else {
                bail!("map.contains first argument must be map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            Ok((true, Some(RuntimeValue::Bool(map.contains_key(&key)))))
        }
        "map.remove" => {
            if args.len() != 2 {
                bail!("map.remove expects arguments (map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::Map(mut map) = args.pop().expect("arg exists") else {
                bail!("map.remove first argument must be map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            map.remove(&key);
            Ok((true, Some(RuntimeValue::Map(map))))
        }
        "map.clear" => {
            if args.len() != 1 {
                bail!("map.clear expects one map<K, V> argument")
            }
            let RuntimeValue::Map(mut map) = args.pop().expect("arg exists") else {
                bail!("map.clear first argument must be map<K, V>")
            };
            map.clear();
            Ok((true, Some(RuntimeValue::Map(map))))
        }
        "map.is_empty" => {
            if args.len() != 1 {
                bail!("map.is_empty expects one map<K, V> argument")
            }
            let RuntimeValue::Map(map) = args.pop().expect("arg exists") else {
                bail!("map.is_empty first argument must be map<K, V>")
            };
            Ok((true, Some(RuntimeValue::Bool(map.is_empty()))))
        }
        "map.len" => {
            if args.len() != 1 {
                bail!("map.len expects one map<K, V> argument")
            }
            let RuntimeValue::Map(map) = args.pop().expect("arg exists") else {
                bail!("map.len first argument must be map<K, V>")
            };
            Ok((true, Some(RuntimeValue::Int(map.len() as i64))))
        }
        "ordered_map.new" => {
            if !args.is_empty() {
                bail!("ordered_map.new expects no arguments")
            }
            Ok((true, Some(RuntimeValue::OrderedMap(BTreeMap::new()))))
        }
        "ordered_map.put" => {
            if args.len() != 3 {
                bail!("ordered_map.put expects arguments (ordered_map<K, V>, K, V)")
            }

            let value = args.pop().expect("arg exists");
            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::OrderedMap(mut map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.put first argument must be ordered_map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            map.insert(key, value);
            Ok((true, Some(RuntimeValue::OrderedMap(map))))
        }
        "ordered_map.get" => {
            if args.len() != 2 {
                bail!("ordered_map.get expects arguments (ordered_map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::OrderedMap(map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.get first argument must be ordered_map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            let value = map
                .get(&key)
                .cloned()
                .ok_or_else(|| anyhow!("ordered_map.get key not found"))?;
            Ok((true, Some(value)))
        }
        "ordered_map.contains" => {
            if args.len() != 2 {
                bail!("ordered_map.contains expects arguments (ordered_map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::OrderedMap(map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.contains first argument must be ordered_map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            Ok((true, Some(RuntimeValue::Bool(map.contains_key(&key)))))
        }
        "ordered_map.remove" => {
            if args.len() != 2 {
                bail!("ordered_map.remove expects arguments (ordered_map<K, V>, K)")
            }

            let key_value = args.pop().expect("arg exists");
            let RuntimeValue::OrderedMap(mut map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.remove first argument must be ordered_map<K, V>")
            };

            let key = runtime_key_from_value(&key_value)?;
            map.remove(&key);
            Ok((true, Some(RuntimeValue::OrderedMap(map))))
        }
        "ordered_map.clear" => {
            if args.len() != 1 {
                bail!("ordered_map.clear expects one ordered_map<K, V> argument")
            }
            let RuntimeValue::OrderedMap(mut map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.clear first argument must be ordered_map<K, V>")
            };
            map.clear();
            Ok((true, Some(RuntimeValue::OrderedMap(map))))
        }
        "ordered_map.is_empty" => {
            if args.len() != 1 {
                bail!("ordered_map.is_empty expects one ordered_map<K, V> argument")
            }
            let RuntimeValue::OrderedMap(map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.is_empty first argument must be ordered_map<K, V>")
            };
            Ok((true, Some(RuntimeValue::Bool(map.is_empty()))))
        }
        "ordered_map.len" => {
            if args.len() != 1 {
                bail!("ordered_map.len expects one ordered_map<K, V> argument")
            }
            let RuntimeValue::OrderedMap(map) = args.pop().expect("arg exists") else {
                bail!("ordered_map.len first argument must be ordered_map<K, V>")
            };
            Ok((true, Some(RuntimeValue::Int(map.len() as i64))))
        }
        _ => Ok((false, None)),
    }
}

fn runtime_key_from_value(value: &RuntimeValue) -> Result<RuntimeKey> {
    match value {
        RuntimeValue::Bool(value) => Ok(RuntimeKey::Bool(*value)),
        RuntimeValue::Int(value) => Ok(RuntimeKey::Int(*value)),
        RuntimeValue::Str(value) => Ok(RuntimeKey::Str(value.clone())),
        _ => bail!("map key must be bool, integer, or str"),
    }
}

pub fn format_value(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::Void => "void".to_string(),
        RuntimeValue::Int(value) => value.to_string(),
        RuntimeValue::Float(value) => value.to_string(),
        RuntimeValue::Bool(value) => value.to_string(),
        RuntimeValue::Str(value) => value.clone(),
        RuntimeValue::Vec(values) => {
            let inner = values
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        RuntimeValue::Map(values) => {
            let mut entries = values
                .iter()
                .map(|(k, v)| format!("{}: {}", format_key(k), format_value(v)))
                .collect::<Vec<_>>();
            entries.sort_unstable();
            format!("{{{}}}", entries.join(", "))
        }
        RuntimeValue::OrderedMap(values) => {
            let entries = values
                .iter()
                .map(|(k, v)| format!("{}: {}", format_key(k), format_value(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{entries}}}")
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

fn format_key(key: &RuntimeKey) -> String {
    match key {
        RuntimeKey::Bool(value) => value.to_string(),
        RuntimeKey::Int(value) => value.to_string(),
        RuntimeKey::Str(value) => format!("\"{}\"", value),
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

    let has_word = str.contains(message, "Sable")
    if has_word {
        io.out(str.slice(message, 0, 5))
    }

    let v: vec<i64> = vec.new()
    v = vec.push(v, 7)
    v = vec.push(v, 35)

    let count = vec.len(v)
    io.out(count)

    let m: map<str, i64> = map.new()
    m = map.put(m, "answer", vec.get(v, 1))

    let om: ordered_map<str, i64> = ordered_map.new()
    om = ordered_map.put(om, "left", 1)
    om = ordered_map.put(om, "right", 2)

    let idx = str.find(message, "Sable")
    let result = map.get(m, "answer") + idx + ordered_map.len(om)
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
        assert_eq!(value, Some(RuntimeValue::Int(44)));
    }

    #[test]
    fn executes_collection_mutation_and_dynamic_for_loops() {
        let src = r#"
fn main() -> i32
    effects(alloc)
{
    let total = 0s

    let text = "Sable"
    for ch in text {
        total += ch
    }

    let values: vec<i32> = vec.new()
    values = vec.push(values, 10s)
    values = vec.push(values, 20s)
    values = vec.push(values, 30s)
    for value in values {
        total += value
    }

    values = vec.remove(values, 1)
    let values_empty_before = vec.is_empty(values)
    let values_len = vec.len(values)
    values = vec.clear(values)
    let values_empty_after = vec.is_empty(values)

    let m: map<str, i32> = map.new()
    m = map.put(m, "a", 1s)
    m = map.put(m, "b", 2s)
    m = map.remove(m, "a")
    let has_a = map.contains(m, "a")
    m = map.clear(m)
    let m_empty = map.is_empty(m)

    let om: ordered_map<str, i32> = ordered_map.new()
    om = ordered_map.put(om, "left", 1s)
    om = ordered_map.put(om, "right", 2s)
    om = ordered_map.remove(om, "left")
    let has_left = ordered_map.contains(om, "left")
    om = ordered_map.clear(om)
    let om_empty = ordered_map.is_empty(om)

    if values_empty_before {
        total += 1000s
    } else {
        total += 1s
    }

    if values_empty_after {
        total += 2s
    }

    if has_a {
        total += 1000s
    } else {
        total += 4s
    }

    if m_empty {
        total += 8s
    }

    if has_left {
        total += 1000s
    } else {
        total += 16s
    }

    if om_empty {
        total += 32s
    }

    if values_len == 2 {
        total += 64s
    }

    return total
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
        assert_eq!(value, Some(RuntimeValue::Int(674)));
    }
}
