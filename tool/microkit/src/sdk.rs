//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::env::{self, VarError};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BoardInfo {
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AvailableConfig {
    pub board_name: String,
    pub board_dir: PathBuf,
    pub config_name: String,
    pub config_dir: PathBuf,
}

pub struct Sdk {
    pub exe_path: PathBuf,
    pub cwd: PathBuf,
    pub sdk_dir: PathBuf,
    pub boards_dir: PathBuf,
    pub available_boards: Vec<BoardInfo>,
    pub available_configs: Vec<AvailableConfig>,
}

#[derive(Debug)]
pub enum SdkInfoError {
    CannotDetermineExePath { source: io::Error },
    CannotDetermineCwd { source: io::Error },
    CannotReadSdkVar { source: VarError },
    CannotInferSdkDir { exe_path: PathBuf },
    CannotFindSdkDirectory { path: PathBuf },
    CannotFindBoardsDirectory { path: PathBuf },
    CannotFindDirectory { path: PathBuf },
    CannotReadDirectory { path: PathBuf, source: io::Error },
}

impl fmt::Display for SdkInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CannotDetermineExePath { source } => {
                write!(
                    f,
                    "Could not determine the current executable path: {source}"
                )
            }
            Self::CannotDetermineCwd { source } => {
                write!(
                    f,
                    "Could not determine the current working directory: {source}"
                )
            }
            Self::CannotReadSdkVar { source } => {
                write!(
                    f,
                    "Could not read the MICROKIT_SDK environmental variable: {source}"
                )
            }
            Self::CannotFindSdkDirectory { path } => {
                write!(
                    f,
                    "The MICROKIT_SDK directory '{}' does not exist",
                    path.display()
                )
            }
            Self::CannotFindBoardsDirectory { path } => {
                write!(
                    f,
                    "The MICROKIT_SDK directory does not have a 'board' sub-directory at '{}'",
                    path.display()
                )
            }
            Self::CannotInferSdkDir { exe_path } => {
                write!(
                    f,
                    "No SDK directory specified and cannot infer SDK directory from executable path '{}'",
                    exe_path.display()
                )
            }
            Self::CannotFindDirectory { path } => {
                write!(f, "The directory '{}' does not exist", path.display())
            }
            Self::CannotReadDirectory { path, source } => {
                write!(
                    f,
                    "The directory '{}' could not be read: {source}",
                    path.display()
                )
            }
        }
    }
}

fn read_dir(path: &Path) -> Result<fs::ReadDir, SdkInfoError> {
    match fs::read_dir(path) {
        Ok(result) => Ok(result),
        Err(ioerr) => Err(SdkInfoError::CannotReadDirectory {
            path: path.to_path_buf(),
            source: ioerr,
        }),
    }
}

fn read_dir_entry(
    path: &Path,
    entry: Result<fs::DirEntry, io::Error>,
) -> Result<fs::DirEntry, SdkInfoError> {
    match entry {
        Ok(result) => Ok(result),
        Err(ioerr) => Err(SdkInfoError::CannotReadDirectory {
            path: path.to_path_buf(),
            source: ioerr,
        }),
    }
}

fn final_path_component(path: &Path) -> Result<&str, SdkInfoError> {
    path.file_name()
        .and_then(|component| component.to_str())
        .ok_or(SdkInfoError::CannotFindDirectory {
            path: path.to_path_buf(),
        })
}

fn discover_sdk_dir(default_path: &Path) -> Result<PathBuf, SdkInfoError> {
    match env::var("MICROKIT_SDK") {
        Ok(value) => {
            // happy path, MICROKIT_SDK is set
            let path = PathBuf::from(value);
            if path.exists() && path.is_dir() {
                Ok(path)
            } else {
                Err(SdkInfoError::CannotFindSdkDirectory { path })
            }
        }
        Err(VarError::NotPresent) => {
            // there is no MICROKIT_SDK explicitly set, use the one that the binary is in
            let grandpa = Some(default_path)
                .and_then(|p| p.parent())
                .and_then(|p| p.parent());
            match grandpa {
                Some(gp) => Ok(gp.to_path_buf()),
                None => Err(SdkInfoError::CannotInferSdkDir {
                    exe_path: default_path.to_path_buf(),
                }),
            }
        }
        Err(source) => {
            // something goes very wrong while reading env variables
            Err(SdkInfoError::CannotReadSdkVar { source })
        }
    }
}

fn discover_boards(dir: &Path) -> Result<Vec<BoardInfo>, SdkInfoError> {
    let entries = read_dir(dir)?;
    let mut discovered = Vec::new();
    for entry in entries {
        let entry = read_dir_entry(dir, entry)?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = final_path_component(&path)?.to_owned();
        let board = BoardInfo { name, dir: path };
        discovered.push(board);
    }
    Ok(discovered)
}

fn discover_available_configs_for(board: &BoardInfo) -> Result<Vec<AvailableConfig>, SdkInfoError> {
    let entries = read_dir(&board.dir)?;
    let mut discovered = Vec::new();
    for entry in entries {
        // TODO: we should defer this check if possible, so that we don't
        // bail when some irrelevant board has a permission error
        let entry = read_dir_entry(&board.dir, entry)?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = final_path_component(&path)?.to_owned();
        if name == "example" {
            continue;
        }
        let config = AvailableConfig {
            board_name: board.name.clone(),
            board_dir: board.dir.clone(),
            config_name: name,
            config_dir: path.to_path_buf(),
        };
        discovered.push(config);
    }
    Ok(discovered)
}

impl Sdk {
    pub fn discover() -> Result<Self, SdkInfoError> {
        let exe_path =
            env::current_exe().map_err(|source| SdkInfoError::CannotDetermineExePath { source })?;
        let cwd =
            env::current_dir().map_err(|source| SdkInfoError::CannotDetermineCwd { source })?;
        let sdk_dir = discover_sdk_dir(&exe_path)?;
        let boards_dir = sdk_dir.join("board");
        if !boards_dir.exists() || !boards_dir.is_dir() {
            return Err(SdkInfoError::CannotFindBoardsDirectory { path: boards_dir });
        }

        let mut available_boards = discover_boards(&boards_dir)?;
        available_boards.sort_by_key(|b| b.name.clone());

        let mut available_configs: Vec<AvailableConfig> = Vec::new();

        for board in &available_boards {
            available_configs.append(&mut discover_available_configs_for(board)?);
        }

        Ok(Self {
            exe_path,
            cwd,
            sdk_dir,
            boards_dir,
            available_boards,
            available_configs,
        })
    }

    pub fn select(&self, board_name: &str, config_name: &str) -> Option<&AvailableConfig> {
        self.available_configs
            .iter()
            .find(|pair| pair.board_name == board_name && pair.config_name == config_name)
    }

    pub fn available_boards_contains(&self, name: &str) -> bool {
        self.available_boards.iter().any(|b| b.name == name)
    }

    pub fn available_board_names(&self) -> Vec<String> {
        self.available_boards
            .iter()
            .map(|b| b.name.clone())
            .collect()
    }

    pub fn available_config_names_for(&self, board_name: &str) -> Vec<String> {
        self.available_configs
            .iter()
            .filter(|c| c.board_name == board_name)
            .map(|c| c.config_name.clone())
            .collect()
    }
}
