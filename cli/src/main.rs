extern crate clap;
extern crate jmespath;

use std::io::prelude::*;
use std::io;
use std::fs::File;
use std::process::exit;
use std::rc::Rc;

use clap::{Arg, App};
use jmespath::{Variable, Expression};

macro_rules! die(
    ($msg:expr) => (
        match writeln!(&mut ::std::io::stderr(), "{}", $msg) {
            Ok(_) => exit(1),
            Err(x) => panic!("Unable to write to stderr: {}", x),
        }
    )
);

fn main() {
    let matches = App::new("jp")
        .version("1.0")
        .author("Michael Dowling <mtdowling@gmail.com>")
        .about("JMESPath command line interface")
        .arg(Arg::with_name("filename")
            .help("Read input JSON from a file instead of stdin.")
            .short("f")
            .takes_value(true)
            .long("filename"))
        .arg(Arg::with_name("unquoted")
            .help("If the final result is a string, it will be printed without quotes.")
            .short("u")
            .long("unquoted")
            .multiple(false))
        .arg(Arg::with_name("ast")
            .help("Only print the AST of the parsed expression.  Do not rely on this output, \
                  only useful for debugging purposes.")
            .long("ast")
            .multiple(false))
        .arg(Arg::with_name("expr-file")
            .help("Read JMESPath expression from the specified file.")
            .short("e")
            .takes_value(true)
            .long("expr-file")
            .conflicts_with("expression")
            .required(true))
        .arg(Arg::with_name("expression")
            .help("JMESPath expression to evaluate")
            .index(1)
            .conflicts_with("expr-file")
            .required(true))
        .get_matches();

    let file_expression = matches.value_of("expr-file")
        .map(|f| read_file("expression", f));

    let expr = if let Some(ref e) = file_expression {
        Expression::new(e)
    } else {
        Expression::new(matches.value_of("expression").unwrap())
    }.map_err(|e| die!(e.to_string())).unwrap();

    if matches.is_present("ast") {
        println!("{}", expr.as_ast());
        exit(0);
    }

    let json = get_json(matches.value_of("filename"));

    match expr.search(json) {
        Err(e) => die!(e.to_string()),
        Ok(result) => show_result(result, matches.is_present("unquoted"))
    }
}

fn show_result(result: Rc<Variable>, unquoted: bool) {
    if unquoted && result.is_string() {
        println!("{}", result.as_string().unwrap());
    } else {
        match result.to_pretty_string() {
            Some(s) => println!("{}", s),
            None => die!(format!("Error converting result to string: {:?}", result)),
        }
    }
}

fn read_file(label: &str, filename: &str) -> String {
    match File::open(filename) {
        Err(e) => die!(format!("Error opening {} file at {}: {}", label, filename, e)),
        Ok(mut file) => {
            let mut buffer = String::new();
            file.read_to_string(&mut buffer)
                .map_err(|e| die!(format!("Error reading {} from {}: {}", label, filename, e)))
                .map(|_| buffer)
                .unwrap()
        }
    }
}

fn get_json(filename: Option<&str>) -> Variable {
    let buffer = match filename {
        Some(f) => read_file("JSON", f),
        None => {
            let mut buffer = String::new();
            match io::stdin().read_to_string(&mut buffer) {
                Ok(_) => buffer,
                Err(e) => die!(format!("Error reading JSON from stdin: {}", e))
            }
        }
    };
    Variable::from_str(&buffer)
        .map_err(|e| die!(format!("Error parsing JSON: {}", e)))
        .unwrap()
}
