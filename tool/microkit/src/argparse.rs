//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::fmt;
use std::iter::Peekable;
use std::path::PathBuf;
use crate::sdkparse::{SdkInfo};

pub fn print_usage() {
    println!("usage: microkit [-h] [OPTIONS] --board BOARD --config CONFIG [--search-path SEARCH_PATH ...] system")
}

pub fn print_help(sdkinfo: &SdkInfo) {
    print_usage();
    println!("\npositional arguments:");
    println!("  system");
    println!("\noptions:");
    println!("  -h, --help, show this help message and exit");
    println!("  -o, --output OUTPUT");
    println!("  -r, --report REPORT");
    println!("  --image-type {{binary,elf,uimage}}");
    println!("  --override-kernel KERNEL (for debugging purposes)");
    println!("  --board {}", sdkinfo.available_board_names().join("\n          "));
    println!("  --config CONFIG");
    println!("  --capdl-json CAPDL_SPEC (JSON format)");
    println!("  --search-path [SEARCH_PATH ...]");
}

#[derive(Debug, Clone)]
pub enum RequestedImageType {
    Binary,
    Elf,
    Uimage,
    Unspecified,
}

impl RequestedImageType {
    fn parse(arg: &str) -> Option<Self> {
        match arg {
            "binary" => Some(RequestedImageType::Binary),
            "elf" => Some(RequestedImageType::Elf),
            "uimage" => Some(RequestedImageType::Uimage),
            _ => None,
        }
    }
}

impl fmt::Display for RequestedImageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestedImageType::Binary => write!(f, "binary"),
            RequestedImageType::Elf => write!(f, "elf"),
            RequestedImageType::Uimage => write!(f, "uimage"),
            RequestedImageType::Unspecified => write!(f, "unspecified"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Args {
    pub sdf_path: PathBuf,
    pub board: String,
    pub config: String,
    pub report_path: PathBuf,
    pub capdl_json_path: Option<PathBuf>,
    pub output_path: PathBuf,
    pub search_paths: Vec<PathBuf>,
    pub requested_image_type: RequestedImageType,
    pub override_kernel: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ArgsError {
    InvalidImageTypeParameter { parameter: String },
    InvalidBoardParameter { parameter: String },
    InvalidConfigParameter { parameter: String, choices: Vec<String> },
    MissingParameter { parent_argument: &'static str },
    MissingRequiredArguments { args: Vec<&'static str> },
    UnrecognizedArgument { arg: String },
    HelpWanted,
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImageTypeParameter { parameter } => {
                write!(f, "argument --image-type: unknown parameter '{parameter}'")
            }
            Self::InvalidBoardParameter { parameter } => {
                write!(f, "argument --board: unknown parameter '{parameter}'")
            }
            Self::InvalidConfigParameter { parameter, choices } => {
                write!(
                    f,
                    "argument --config: invalid choice '{parameter}' (choose from: {})"
                    , choices.join(", ")
                )
            }
            Self::MissingParameter { parent_argument } => {
                write!(f, "argument {parent_argument}: expected one parameter")
            }
            Self::MissingRequiredArguments { args } => {
                write!(
                    f,
                    "the following arguments are required: {}",
                    args.join(", ")
                )
            }
            Self::UnrecognizedArgument { arg } => {
                write!(f, "unrecognized argument '{arg}'")
            }
            Self::HelpWanted => {
                write!(f, "printing help text")
            }
        }
    }
}

fn consume_parameter<I>(args: &mut I, argname: &'static str) -> Result<String, ArgsError>
where
    I: Iterator<Item = String>,
{
    args.next().ok_or(ArgsError::MissingParameter {
        parent_argument: argname,
    })
}

fn consume_parameters<I>(args: &mut Peekable<I>) -> Vec<String>
where
    I: Iterator<Item = String>,
{
    let mut values = Vec::new();
    while let Some(next) = args.peek() {
        if next.starts_with("-") {
            break;
        }
        if let Some(next) = args.next() {
            values.push(next)
        };
    }
    values
}

impl Args {
    pub fn parse(args: &[String], sdkinfo: &SdkInfo) -> Result<Self, ArgsError> {
        let mut args = args.iter().skip(1).cloned().peekable();

        let mut output_path = PathBuf::from("loader.img");
        let mut report_path = PathBuf::from("report.txt");
        let mut capdl_json_path = None;
        let mut search_paths = Vec::new();

        let mut sdf_path = None;
        let mut board = None;
        let mut config = None;
        let mut requested_image_type = RequestedImageType::Unspecified;
        let mut override_kernel = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    return Err(ArgsError::HelpWanted);
                }
                "-o" | "--output" => {
                    output_path = consume_parameter(&mut args, "--output")?.into();
                }
                "-r" | "--report" => {
                    report_path = consume_parameter(&mut args, "--report")?.into();
                }
                "--board" => {
                    let board_param = consume_parameter(&mut args, "--board")?;
                    if !sdkinfo.available_boards_contains(&board_param) {
                        return Err(ArgsError::InvalidBoardParameter {
                            parameter: board_param,
                        });
                    }
                    board = Some(board_param);
                }
                "--config" => {
                    config = Some(consume_parameter(&mut args, "--config")?);
                }
                "--capdl-json" => {
                    capdl_json_path = Some(consume_parameter(&mut args, "--capdl-json")?.into());
                }
                "--search-path" => {
                    let params = consume_parameters(&mut args);
                    search_paths.extend(params.into_iter().map(PathBuf::from));
                }
                "--image-type" => {
                    let value = consume_parameter(&mut args, "--image-type")?;
                    match RequestedImageType::parse(value.as_str()) {
                        Some(image_type) => {
                            requested_image_type = image_type;
                        }
                        None => {
                            return Err(ArgsError::InvalidImageTypeParameter { parameter: value });
                        }
                    }
                }
                "--override-kernel" => {
                    override_kernel =
                        Some(consume_parameter(&mut args, "--override-kernel")?.into());
                }
                value => {
                    if sdf_path.is_none() {
                        sdf_path = Some(value.into());
                    } else {
                        return Err(ArgsError::UnrecognizedArgument {
                            arg: value.to_owned(),
                        });
                    }
                }
            }
        }

        let mut missing_args = Vec::new();
        if board.is_none() {
            missing_args.push("--board");
        }
        if config.is_none() {
            missing_args.push("--config");
        }
        if sdf_path.is_none() {
            missing_args.push("system");
        }
        if !missing_args.is_empty() {
            return Err(ArgsError::MissingRequiredArguments { args: missing_args });
        }
        let board = board.unwrap();
        let config = config.unwrap();
        let sdf_path = sdf_path.unwrap();

        if sdkinfo.select(&board, &config).is_none() {
            return Err(
                ArgsError::InvalidConfigParameter {
                    parameter: config,
                    choices: sdkinfo.available_config_names_for(&board),
                }
            );
        }

        Ok(Self {
            sdf_path,
            board,
            config,
            report_path,
            capdl_json_path,
            output_path,
            search_paths,
            requested_image_type,
            override_kernel,
        })
    }
}
