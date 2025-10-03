mod args;
mod check;

use args::Args;
use check::{ItemError, validate_with};
use clap::Parser;
use std::error::Error;
use std::path::Path;

fn criteria(item: &str) -> Result<String, ItemError> {
    if is_valid_directory(item) {
        Ok(item.to_string())
    } else {
        Err(ItemError {
            item: item.to_string(),
            message: "No such directory found.".to_string(),
        })
    }
}

fn is_valid_directory(path: &str) -> bool {
    return Path::new(path).is_dir();
}

fn main() -> Result<(), Box<dyn Error>> {
    let produced = Args::parse().produce_links();
    let links: Vec<&str> = produced.iter().map(|s| s.as_str()).collect();
    let result = validate_with(links, criteria);

    println!("Valid items {:?}", result.valid);

    println!("Invalid items:");
    for error in result.invalid {
        println!(" - {}", error);
    }

    Ok(())
}
