//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use microkit_tool::build::{self, build};
use microkit_tool::sdf::parse;
use microkit_tool::sel4::{get_available_boards, Config};
use std::fs::{self};
use std::path::{Path, PathBuf};

fn print_usage() {
    println!("usage: microkit [-h] [-o OUTPUT] [--image-type {{binary,elf,uimage}}] [-r REPORT] --board BOARD --config CONFIG [--capdl-json CAPDL_SPEC] --search-path [SEARCH_PATH ...] system")
}

fn print_help(available_boards: &[String]) {
    print_usage();
    println!("\npositional arguments:");
    println!("  system");
    println!("\noptions:");
    println!("  -h, --help, show this help message and exit");
    println!("  -o, --output OUTPUT");
    println!("  -r, --report REPORT");
    println!("  --image-type {{binary,elf}}");
    println!("  --board {}", available_boards.join("\n          "));
    println!("  --config CONFIG");
    println!("  --capdl-json CAPDL_SPEC (JSON format)");
    println!("  --search-path [SEARCH_PATH ...]");
}

struct Args<'a> {
    system: &'a str,
    board: &'a str,
    config: &'a str,
    report: &'a str,
    capdl_json: Option<&'a str>,
    output: &'a str,
    search_paths: Vec<&'a String>,
    output_image_type: Option<&'a str>,
}

impl<'a> Args<'a> {
    pub fn parse(args: &'a [String], available_boards: &[String]) -> Args<'a> {
        // Default arguments
        let mut output = "loader.img";
        let mut report = "report.txt";
        let mut capdl_json = None;
        let mut search_paths = Vec::new();
        // Arguments expected to be provided by the user
        let mut system = None;
        let mut board = None;
        let mut config = None;
        let mut output_image_type = None;

        if args.len() <= 1 {
            print_usage();
            std::process::exit(1);
        }

        let mut i = 1;
        let mut unknown = vec![];
        let mut in_search_path = false;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => {
                    print_help(available_boards);
                    std::process::exit(0);
                }
                "-o" | "--output" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        output = &args[i + 1];
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument -o/--output: expected one argument");
                        std::process::exit(1);
                    }
                }
                "-r" | "--report" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        report = &args[i + 1];
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument -r/--report: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--board" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        board = Some(&args[i + 1]);
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --board: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--config" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        config = Some(&args[i + 1]);
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --config: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--capdl-json" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        capdl_json = Some(args[i + 1].as_str());
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --capdl-json: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--search-path" => {
                    in_search_path = true;
                }
                "--image-type" => {
                    if i < args.len() - 1 {
                        output_image_type = Some(args[i + 1].as_str());
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --image-type: expected one argument");
                        std::process::exit(1);
                    }
                }
                _ => {
                    if in_search_path {
                        search_paths.push(&args[i]);
                    } else if system.is_none() {
                        system = Some(&args[i]);
                    } else {
                        // This call to clone is okay since having unknown
                        // arguments is rare.
                        unknown.push(args[i].clone());
                    }
                }
            }

            i += 1;
        }

        if !unknown.is_empty() {
            print_usage();
            eprintln!(
                "microkit: error: unrecognised arguments: {}",
                unknown.join(" ")
            );
            std::process::exit(1);
        }

        let mut missing_args = Vec::new();
        if board.is_none() {
            missing_args.push("--board");
        }
        if config.is_none() {
            missing_args.push("--config");
        }
        if system.is_none() {
            missing_args.push("system");
        }

        if !missing_args.is_empty() {
            print_usage();
            eprintln!(
                "microkit: error: the following arguments are required: {}",
                missing_args.join(", ")
            );
            std::process::exit(1);
        }

        Args {
            system: system.unwrap(),
            board: board.unwrap(),
            config: config.unwrap(),
            report,
            capdl_json,
            output,
            search_paths,
            output_image_type,
        }
    }
}

fn main() -> Result<(), String> {
    let exe_path = std::env::current_exe().unwrap();
    let sdk_env = std::env::var("MICROKIT_SDK");
    let sdk_dir = match sdk_env {
        Ok(ref value) => Path::new(value),
        Err(err) => match err {
            // If there is no MICROKIT_SDK explicitly set, use the one that the binary is in.
            std::env::VarError::NotPresent => exe_path.parent().unwrap().parent().unwrap(),
            _ => {
                return Err(format!(
                    "Could not read MICROKIT_SDK environment variable: {err}"
                ))
            }
        },
    };

    if !sdk_dir.exists() {
        eprintln!(
            "Error: SDK directory '{}' does not exist.",
            sdk_dir.display()
        );
        std::process::exit(1);
    }

    let available_boards = get_available_boards(sdk_dir).unwrap();
    let env_args: Vec<_> = std::env::args().collect();
    let args = Args::parse(&env_args, &available_boards);

    let kernel_config =
        Config::from_board_profile(&args.board.to_string(), &args.config.to_string(), sdk_dir)
            .unwrap();

    let xml: String = fs::read_to_string(args.system).unwrap();

    let mut search_paths = vec![std::env::current_dir().unwrap()];
    for path in args.search_paths {
        search_paths.push(PathBuf::from(path));
    }

    let mut system = match parse(args.system, &xml, &kernel_config, &search_paths) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let _ = build(
        &kernel_config,
        &args.board.to_string(),
        &args.config.to_string(),
        &mut system,
        sdk_dir,
        search_paths,
        &args.output.to_string(),
        &args.report.to_string(),
        args.capdl_json.into(),
        args.output_image_type,
    );

    Ok(())
}
