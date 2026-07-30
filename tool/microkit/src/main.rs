//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use microkit_tool::argparse;
use microkit_tool::argparse::{Args, ArgsError};
use microkit_tool::build::build_system;
use microkit_tool::sdf::parse;
use microkit_tool::sdk::Sdk;
use microkit_tool::sel4::Config;
use microkit_tool::util::bail_if_not_exists;
use std::fs;

fn main() -> Result<(), String> {
    let sdk = match Sdk::discover() {
        Ok(discovered_info) => discovered_info,
        Err(err) => {
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    };

    let env_args: Vec<_> = std::env::args().collect();
    let mut args = match Args::parse(&env_args, &sdk) {
        Ok(parsed_arguments) => parsed_arguments,
        Err(ArgsError::HelpWanted) => {
            argparse::print_help(&sdk);
            std::process::exit(0);
        }
        Err(err) => {
            match err {
                ArgsError::UnrecognizedArgument { arg: _ }
                | ArgsError::MissingRequiredArguments { args: _ } => {
                    argparse::print_usage();
                }
                _ => {}
            };
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    };

    args.search_paths.push(sdk.cwd.clone());

    let kernel_config = Config::build_kernel_config(&args, &sdk).unwrap();

    let system_path = &args.sdf_path;
    bail_if_not_exists("system description file", system_path)?;

    let xml: String = fs::read_to_string(system_path).unwrap();

    let mut system = match parse(
        system_path.as_path(),
        &xml,
        &kernel_config,
        &args.search_paths,
    ) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    build_system(&args, &sdk, &kernel_config, &mut system)
}
