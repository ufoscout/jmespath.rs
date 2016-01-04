use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::cmp::{max, min};
use std::fmt;

use super::RuntimeError;
use super::interpreter::{TreeInterpreter, SearchResult};
use super::variable::Variable;

/// Represents an argument type
#[derive(Clone,Debug,PartialEq)]
pub enum ArgumentType {
    Any,
    String,
    Number,
    Bool,
    Array,
    Object,
    Null,
    Expref,
    HomogeneousArray(Vec<ArgumentType>),
    OneOf(Vec<ArgumentType>),
    ExprefReturns(Vec<ArgumentType>)
}

impl ArgumentType {
    /// Convert a Vec of types to Vec<String>
    pub fn types_to_strings(types: &Vec<ArgumentType>) -> Vec<String> {
        types.iter().map(|t| t.to_string()).collect::<Vec<String>>()
    }

    /// Returns true/false if the variable is valid for the type.
    pub fn is_valid(&self, value: &Rc<Variable>) -> bool {
        use self::ArgumentType::*;
        match self {
            &Any => true,
            &Null if value.is_null() => true,
            &String if value.is_string() => true,
            &Number if value.is_number() => true,
            &Object if value.is_object() => true,
            &Bool if value.is_boolean() => true,
            &Expref if value.is_expref() => true,
            &ExprefReturns(_) if value.is_expref() => true,
            &Array if value.is_array() => true,
            &OneOf(ref types) => types.iter().any(|t| t.is_valid(value)),
            &HomogeneousArray(ref types) if value.is_array() => {
                let values = value.as_array().unwrap();
                if values.is_empty() {
                    true
                } else {
                    let alts = OneOf(types.clone());
                    let first_type = values[0].get_type();
                    values.iter().all(|v| alts.is_valid(v) && v.get_type() == first_type)
                }
            },
            _ => false
        }
    }
}

impl fmt::Display for ArgumentType {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use self::ArgumentType::*;
        match self {
            &Any => write!(fmt, "any"),
            &String => write!(fmt, "string"),
            &Number => write!(fmt, "number"),
            &Bool => write!(fmt, "boolean"),
            &Array => write!(fmt, "array"),
            &Object => write!(fmt, "object"),
            &Null => write!(fmt, "null"),
            &Expref => write!(fmt, "expref"),
            &ExprefReturns(ref types) => {
                let mut type_strings = vec![];
                for t in types {
                    type_strings.push(format!("expref->{}", t));
                }
                write!(fmt, "{}", type_strings.join("|"))
            },
            &OneOf(ref types) => write!(fmt, "{}", Self::types_to_strings(types).join("|")),
            &HomogeneousArray(ref types) => {
                write!(fmt, "array[{}]", Self::types_to_strings(types).join("|"))
            }
        }
    }
}

/// JMESPath function
pub trait JPFunction {
    /// Evaluates a function with the given arguments
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult;
}

/// Boxed JPFunction
pub type FnBox = Box<JPFunction + 'static>;

/// Map of JMESPath function names to their implementation
pub type Functions = HashMap<String, FnBox>;

/// Validates the arity of a function.
#[inline]
pub fn validate_arity(expected: usize, actual: usize) -> Result<(), RuntimeError> {
    if actual == expected {
        Ok(())
    } else if actual < expected {
        Err(RuntimeError::NotEnoughArguments { expected: expected, actual: actual })
    } else {
        Err(RuntimeError::TooManyArguments { expected: expected, actual: actual })
    }
}

/// Validates the arity of a function.
#[inline]
pub fn validate_min_arity(expected: usize, actual: usize) -> Result<(), RuntimeError> {
    if actual < expected {
        Err(RuntimeError::NotEnoughArguments { expected: expected, actual: actual })
    } else {
        Ok(())
    }
}

/// Macro used to variadically validate validate Variable and argument arity.
#[macro_export]
macro_rules! validate {
    // Validate positional arguments only.
    ($args:expr, $($x:expr),*) => (
        {
            let arg_types: Vec<ArgumentType> = vec![$($x), *];
            try!(validate_arity(arg_types.len(), $args.len()));
            for (k, v) in $args.iter().enumerate() {
                if !arg_types[k].is_valid(v) {
                    return Err(RuntimeError::InvalidType {
                        expected: arg_types[k].to_string(),
                        actual: v.get_type().to_string(),
                        actual_value: v.clone(),
                        position: k
                    });
                }
            }
        }
    );
    // Validate positional arguments with a variadic validator.
    ($args:expr, $($x:expr),* ...$variadic:expr ) => (
        {
            let arg_types: Vec<ArgumentType> = vec![$($x), *];
            let variadic = $variadic;
            try!(validate_min_arity(arg_types.len(), $args.len()));
            for (k, v) in $args.iter().enumerate() {
                let validator = arg_types.get(k).unwrap_or(&variadic);
                if !validator.is_valid(v) {
                    return Err(RuntimeError::InvalidType {
                        expected: validator.to_string(),
                        actual: v.get_type().to_string(),
                        actual_value: v.clone(),
                        position: k
                    });
                }
            }
        }
    );
}

/// Macro used to implement max_by and min_by functions.
macro_rules! min_and_max_by {
    ($operator:ident, $args:expr, $interpreter:expr) => (
        {
            validate!($args, ArgumentType::Array, ArgumentType::Expref);
            let vals = $args[0].as_array().unwrap();
            // Return null when there are not values in the array
            if vals.is_empty() {
                return Ok($interpreter.arena.alloc_null());
            }
            let ast = $args[1].as_expref().unwrap();
            // Map over the first value to get the homogeneous required return type
            let initial = try!($interpreter.interpret(vals[0].clone(), &ast));
            let entered_type = initial.get_type();
            if entered_type != "string" && entered_type != "number" {
                return Err(RuntimeError::InvalidReturnType {
                    expected: "expression->number|expression->string".to_string(),
                    actual: entered_type.to_string(),
                    actual_value: initial.clone(),
                    position: 1,
                    invocation: 1
                });
            }
            // Map over each value, finding the best candidate value and fail on error.
            let mut candidate = (vals[0].clone(), initial.clone());
            for (invocation, v) in vals.iter().enumerate().skip(1) {
                let mapped = try!($interpreter.interpret(v.clone(), &ast));
                if mapped.get_type() != entered_type {
                    return Err(RuntimeError::InvalidReturnType {
                        expected: format!("expression->{}", entered_type),
                        actual: mapped.get_type().to_string(),
                        actual_value: mapped.clone(),
                        position: 1,
                        invocation: invocation
                    });
                }
                if mapped.$operator(&candidate.1) {
                    candidate = (v.clone(), mapped);
                }
            }
            Ok(candidate.0)
        }
    )
}

/// Macro used to implement max and min functions.
macro_rules! min_and_max {
    ($operator:ident, $args:expr, $interpreter:expr) => (
        {
            let acceptable = vec![ArgumentType::String, ArgumentType::Number];
            validate!($args, ArgumentType::HomogeneousArray(acceptable));
            let values = $args[0].as_array().unwrap();
            if values.is_empty() {
                Ok($interpreter.arena.alloc_null())
            } else {
                let result: Rc<Variable> = values
                    .iter()
                    .skip(1)
                    .fold(values[0].clone(), |acc, item| $operator(acc, item.clone()));
                Ok(result)
            }
        }
    )
}

/// Registers the default JMESPath functions into a map.
pub fn register_core_functions(functions: &mut Functions) {
    functions.insert("abs".to_string(), Box::new(Abs));
    functions.insert("avg".to_string(), Box::new(Avg));
    functions.insert("ceil".to_string(), Box::new(Ceil));
    functions.insert("contains".to_string(), Box::new(Contains));
    functions.insert("ends_with".to_string(), Box::new(EndsWith));
    functions.insert("floor".to_string(), Box::new(Floor));
    functions.insert("join".to_string(), Box::new(Join));
    functions.insert("keys".to_string(), Box::new(Keys));
    functions.insert("length".to_string(), Box::new(Length));
    functions.insert("map".to_string(), Box::new(Map));
    functions.insert("min".to_string(), Box::new(Min));
    functions.insert("max".to_string(), Box::new(Max));
    functions.insert("max_by".to_string(), Box::new(MaxBy));
    functions.insert("min_by".to_string(), Box::new(MinBy));
    functions.insert("merge".to_string(), Box::new(Merge));
    functions.insert("not_null".to_string(), Box::new(NotNull));
    functions.insert("reverse".to_string(), Box::new(Reverse));
    functions.insert("sort".to_string(), Box::new(Sort));
    functions.insert("sort_by".to_string(), Box::new(SortBy));
    functions.insert("starts_with".to_string(), Box::new(StartsWith));
    functions.insert("sum".to_string(), Box::new(Sum));
    functions.insert("to_array".to_string(), Box::new(ToArray));
    functions.insert("to_number".to_string(), Box::new(ToNumber));
    functions.insert("to_string".to_string(), Box::new(ToString));
    functions.insert("type".to_string(), Box::new(Type));
    functions.insert("values".to_string(), Box::new(Values));
}

struct Abs;

impl JPFunction for Abs {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate![args, ArgumentType::Number];
        match *args[0] {
            Variable::I64(n) => Ok(intr.arena.alloc(n.abs())),
            Variable::F64(f) => Ok(intr.arena.alloc(f.abs())),
            _ => Ok(args[0].clone())
        }
    }
}

struct Avg;

impl JPFunction for Avg {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::HomogeneousArray(vec![ArgumentType::Number]));
        let values = args[0].as_array().unwrap();
        let sum = values.iter()
            .map(|n| n.as_f64().unwrap())
            .fold(0f64, |a, ref b| a + b);
        Ok(intr.arena.alloc(sum / (values.len() as f64)))
    }
}

struct Ceil;

impl JPFunction for Ceil {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Number);
        let n = args[0].as_f64().unwrap();
        Ok(intr.arena.alloc(n.ceil()))
    }
}

struct Contains;

impl JPFunction for Contains {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::OneOf(vec![ArgumentType::String, ArgumentType::Array]),
            ArgumentType::Any);
        let ref haystack = args[0];
        let ref needle = args[1];
        match **haystack {
           Variable::Array(ref a) => Ok(intr.arena.alloc_bool(a.contains(&needle))),
           Variable::String(ref subj) => {
               match needle.as_string() {
                   None => Ok(intr.arena.alloc_bool(false)),
                   Some(s) => Ok(intr.arena.alloc_bool(subj.contains(s)))
               }
           },
           _ => unreachable!()
        }
    }
}

struct EndsWith;

impl JPFunction for EndsWith {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::String, ArgumentType::String);
        let subject = args[0].as_string().unwrap();
        let search = args[1].as_string().unwrap();
        Ok(intr.arena.alloc_bool(subject.ends_with(search)))
    }
}

struct Floor;

impl JPFunction for Floor {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Number);
        let n = args[0].as_f64().unwrap();
        Ok(intr.arena.alloc(n.floor()))
    }
}

struct Join;

impl JPFunction for Join {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::String,
            ArgumentType::HomogeneousArray(vec![ArgumentType::String]));
        let glue = args[0].as_string().unwrap();
        let values = args[1].as_array().unwrap();
        let result = values.iter()
            .map(|v| v.as_string().unwrap())
            .cloned()
            .collect::<Vec<String>>()
            .join(&glue);
        Ok(intr.arena.alloc(result))
    }
}

struct Keys;

impl JPFunction for Keys {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Object);
        let object = args[0].as_object().unwrap();
        let keys = object.keys()
            .map(|k| intr.arena.alloc((*k).clone()))
            .collect::<Vec<Rc<Variable>>>();
        Ok(intr.arena.alloc(keys))
    }
}

struct Length;

impl JPFunction for Length {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        let acceptable = vec![ArgumentType::Array, ArgumentType::Object, ArgumentType::String];
        validate!(args, ArgumentType::OneOf(acceptable));
        match *args[0] {
            Variable::Array(ref a) => Ok(intr.arena.alloc(a.len())),
            Variable::Object(ref m) => Ok(intr.arena.alloc(m.len())),
            // Note that we need to count the code points not the number of unicode characters
            Variable::String(ref s) => Ok(intr.arena.alloc(s.chars().count())),
            _ => unreachable!()
        }
    }
}

struct Map;

impl JPFunction for Map {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Expref, ArgumentType::Array);
        let ast = args[0].as_expref().unwrap();
        let values = args[1].as_array().unwrap();
        let mut results = vec![];
        for value in values {
            results.push(try!(intr.interpret(value.clone(), &ast)));
        }
        Ok(intr.arena.alloc(results))
    }
}

struct Max;

impl JPFunction for Max {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        min_and_max!(max, args, intr)
    }
}

struct Min;

impl JPFunction for Min {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        min_and_max!(min, args, intr)
    }
}

struct MaxBy;

impl JPFunction for MaxBy {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        min_and_max_by!(gt, args, intr)
    }
}

struct MinBy;

impl JPFunction for MinBy {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        min_and_max_by!(lt, args, intr)
    }
}

struct Merge;

impl JPFunction for Merge {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Object ...ArgumentType::Object);
        let mut result = BTreeMap::new();
        for arg in args {
            result.extend(arg.as_object().unwrap().clone());
        }
        Ok(intr.arena.alloc(result))
    }
}

struct NotNull;

impl JPFunction for NotNull {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Any ...ArgumentType::Any);
        for arg in args {
            if !arg.is_null() {
                return Ok(arg.clone());
            }
        }
        Ok(intr.arena.alloc_null())
    }
}

struct Reverse;

impl JPFunction for Reverse {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::OneOf(vec![ArgumentType::Array, ArgumentType::String]));
        if args[0].is_array() {
            let mut values = args[0].as_array().unwrap().clone();
            values.reverse();
            Ok(intr.arena.alloc(values))
        } else {
            let word: String = args[0].as_string().unwrap().chars().rev().collect();
            Ok(intr.arena.alloc(word))
        }
    }
}

struct Sort;

impl JPFunction for Sort {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        let acceptable = vec![ArgumentType::String, ArgumentType::Number];
        validate!(args, ArgumentType::HomogeneousArray(acceptable));
        let mut values = args[0].as_array().unwrap().clone();
        values.sort();
        Ok(intr.arena.alloc(values))
    }
}

struct SortBy;

impl JPFunction for SortBy {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Array, ArgumentType::Expref);
        let vals = args[0].as_array().unwrap().clone();
        if vals.is_empty() {
            return Ok(intr.arena.alloc(vals));
        }
        let ast = args[1].as_expref().unwrap();
        let mut mapped: Vec<(Rc<Variable>, Rc<Variable>)> = vec![];
        let first_value = try!(intr.interpret(vals[0].clone(), &ast));
        let first_type = first_value.get_type();
        if first_type != "string" && first_type != "number" {
            return Err(RuntimeError::InvalidReturnType {
                expected: "expression->string|expression->number".to_string(),
                actual: first_type.to_string(),
                actual_value: first_value.clone(),
                position: 1,
                invocation: 1
            });
        }
        mapped.push((vals[0].clone(), first_value.clone()));
        for (invocation, v) in vals.iter().enumerate().skip(1) {
            let mapped_value = try!(intr.interpret((*v).clone(), &ast));
            if mapped_value.get_type() != first_type {
                return Err(RuntimeError::InvalidReturnType {
                    expected: format!("expression->{}", first_type),
                    actual: mapped_value.get_type().to_string(),
                    actual_value: mapped_value.clone(),
                    position: 1,
                    invocation: invocation
                });
            }
            mapped.push((v.clone(), mapped_value));
        }
        mapped.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(intr.arena.alloc(vals))
    }
}

struct StartsWith;

impl JPFunction for StartsWith {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::String, ArgumentType::String);
        let subject = args[0].as_string().unwrap();
        let search = args[1].as_string().unwrap();
        Ok(intr.arena.alloc_bool(subject.starts_with(search)))
    }
}

struct Sum;

impl JPFunction for Sum {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::HomogeneousArray(vec![ArgumentType::Number]));
        let result = args[0].as_array().unwrap().iter().fold(
            0.0, |acc, item| acc + item.as_f64().unwrap());
        Ok(intr.arena.alloc(result))
    }
}

struct ToArray;

impl JPFunction for ToArray {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Any);
        match *args[0] {
            Variable::Array(_) => Ok(args[0].clone()),
            _ => Ok(intr.arena.alloc(vec![args[0].clone()]))
        }
    }
}

struct ToNumber;

impl JPFunction for ToNumber {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Any);
        match *args[0] {
            Variable::I64(_) | Variable::F64(_) | Variable::U64(_) => Ok(args[0].clone()),
            Variable::String(ref s) => {
                match Variable::from_str(s) {
                    Ok(f)  => Ok(intr.arena.alloc(f)),
                    Err(_) => Ok(intr.arena.alloc_null())
                }
            },
            _ => Ok(intr.arena.alloc_null())
        }
    }
}

struct ToString;

impl JPFunction for ToString {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::OneOf(vec![
            ArgumentType::Object, ArgumentType::Array, ArgumentType::Bool,
            ArgumentType::Number, ArgumentType::String, ArgumentType::Null]));
        match *args[0] {
            Variable::String(_) => Ok(args[0].clone()),
            _ => Ok(intr.arena.alloc(args[0].to_string().unwrap()))
        }
    }
}

struct Type;

impl JPFunction for Type {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Any);
        Ok(intr.arena.alloc(args[0].get_type().to_string()))
    }
}

struct Values;

impl JPFunction for Values {
    fn evaluate(&self, args: Vec<Rc<Variable>>, intr: &TreeInterpreter) -> SearchResult {
        validate!(args, ArgumentType::Object);
        let map = args[0].as_object().unwrap();
        Ok(intr.arena.alloc(map.values().cloned().collect::<Vec<Rc<Variable>>>()))
    }
}
