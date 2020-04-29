# JMESPatCh for Rust

Rust implementation of [JMESPath](http://jmespath.org), a query language for JSON.

[Documentation](https://docs.rs/jmespath/)

# Difference with Jmespath (note path!=patch)
This repository was created with the only purpose of preparing a PR for the original jmespath.rs library.

However, after months of waiting for someone to comment on my PR, it is clear that the jmespath.rs project is in a dead state.

Consequently, I was forced to publish my fix to crates.io with the name **jmespatch**.

Compared to the original code, **jmespatch**:
- upgrades the code to rust 2018 edition
- replaces many .unwrap() calls with code that returns a Result to avoid panics when serde_json is not able to serialize/deserialize
- modifies the Variable::Number to use a serde_json::Number instead of an f64 to mimic the same behavior of serde itself. This permits to use, whenever possible, u64 or i64 instead of f64
- fixes an issue that does not allow to use jmespath.rs with serde_json 1.0.45 or greater
- revamps the project structure to a cargo workspace
- fixes a failing test in jmespath-cli
- fixes all clippy warnings
- updates all dependencies
- modifies the benches to run on stable (no need any more for a nightly compiler to run cargo bench)

## Status of this project
I will keep it until any sign of life is provided by the original jmespath.rs project.

I will not add new features, but every PR submitted is warmly welcomed.


## Installing

This crate is [on crates.io](https://crates.io/crates/jmespatch) and can be used
by adding `jmespatch` to the dependencies in your project's `Cargo.toml`.

```toml
[dependencies]
jmespatch = "0.3.0"
```

## Examples

```rust
extern crate jmespatch;

let expr = jmespatch::compile("foo.bar").unwrap();

// Parse some JSON data into a JMESPath variable
let json_str = r#"{"foo": {"bar": true}}"#;
let data = jmespatch::Variable::from_json(json_str).unwrap();

// Search the data with the compiled expression
let result = expr.search(data).unwrap();
assert_eq!(true, result.as_boolean().unwrap());
```
