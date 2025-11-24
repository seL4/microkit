//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use microkit_tool::capdl::allocation::{
    simulate_capdl_object_alloc_algorithm, CapDLAllocEmulationErrorLevel,
};
use microkit_tool::capdl::build_capdl_spec;
use microkit_tool::capdl::initialiser::CapDLInitialiser;
use microkit_tool::capdl::packaging::pack_spec_into_initial_task;
use microkit_tool::elf::ElfFile;
use microkit_tool::loader::Loader;
use microkit_tool::report::write_report;
use microkit_tool::sdf::{parse, SysMemoryRegion, SysMemoryRegionPaddr};
use microkit_tool::sel4::{
    emulate_kernel_boot, emulate_kernel_boot_partial, Arch, Config, PlatformConfig,
    RiscvVirtualMemory,
};
use microkit_tool::symbols::patch_symbols;
use microkit_tool::util::{
    human_size_strict, json_str, json_str_as_bool, json_str_as_u64, round_down, round_up,
};
use microkit_tool::{DisjointMemoryRegion, MemoryRegion};
use std::collections::HashMap;
use std::fs::{self, metadata};
use std::path::{Path, PathBuf};

const MAX_BUILD_ITERATION: usize = 3;

// When building for x86, the kernel is copied from the SDK release package to the same
// directory as the output boot module image, as Multiboot want them as
// separate images.
const KERNEL_COPY_FILENAME: &str = "sel4.elf";
// The `-kernel` argument of 'qemu-system-x86_64' doesn't accept a 64-bit image, so we
// also copy the 32-bit version that was prepared by build_sdk.py for convenience.
const KERNEL32_COPY_FILENAME: &str = "sel4_32.elf";

fn get_full_path(path: &Path, search_paths: &Vec<PathBuf>) -> Option<PathBuf> {
    for search_path in search_paths {
        let full_path = search_path.join(path);
        if full_path.exists() {
            return Some(full_path.to_path_buf());
        }
    }

    None
}

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

enum ImageOutputType {
    Binary,
    Elf,
    Uimage,
}

impl ImageOutputType {
    fn default_from_arch(arch: &Arch) -> Self {
        match arch {
            Arch::Aarch64 => ImageOutputType::Binary,
            Arch::Riscv64 => ImageOutputType::Uimage,
            Arch::X86_64 => ImageOutputType::Elf,
        }
    }

    fn parse(str: &str, board_name: &str, arch: Arch) -> Result<Self, String> {
        match board_name {
            "ariane" | "cheshire" | "serengeti" => Ok(ImageOutputType::Binary),
            _ => match str {
                "binary" => match arch {
                    Arch::Aarch64 | Arch::Riscv64 => Ok(ImageOutputType::Binary),
                    Arch::X86_64 => Err(format!(
                        "building the output image as binary is unsupported for target architecture '{arch}'"
                    )),
                },
                "elf" => Ok(ImageOutputType::Elf),
                "uimage" => match arch {
                    Arch::Riscv64 => Ok(ImageOutputType::Uimage),
                    Arch::X86_64 | Arch::Aarch64 => Err(format!(
                        "building the output image as uImage is unsupported for target architecture '{arch}'"
                    )),
                },
                _ => Err(format!("unknown value '{str}'")),
            },
        }
    }
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

    let boards_path = sdk_dir.join("board");
    if !boards_path.exists() || !boards_path.is_dir() {
        eprintln!(
            "Error: SDK directory '{}' does not have a 'board' sub-directory.",
            sdk_dir.display()
        );
        std::process::exit(1);
    }

    let mut available_boards = Vec::new();
    for p in fs::read_dir(&boards_path).unwrap() {
        let path_buf = p.unwrap().path();
        let path = path_buf.as_path();
        if path.is_dir() {
            available_boards.push(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }
    available_boards.sort();

    let env_args: Vec<_> = std::env::args().collect();
    let args = Args::parse(&env_args, &available_boards);

    let board_path = boards_path.join(args.board);
    if !board_path.exists() {
        eprintln!(
            "Error: board path '{}' does not exist.",
            board_path.display()
        );
        std::process::exit(1);
    }

    let mut available_configs = Vec::new();
    for p in fs::read_dir(board_path).unwrap() {
        let path_buf = p.unwrap().path();
        let path = path_buf.as_path();

        if path.file_name().unwrap() == "example" {
            continue;
        }

        if path.is_dir() {
            available_configs.push(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }

    if !available_configs.contains(&args.config.to_string()) {
        eprintln!(
            "microkit: error: argument --config: invalid choice: '{}' (choose from: {})",
            args.config,
            available_configs.join(", ")
        )
    }

    let elf_path = sdk_dir
        .join("board")
        .join(args.board)
        .join(args.config)
        .join("elf");
    let loader_elf_path = elf_path.join("loader.elf");
    let kernel_elf_path = elf_path.join("sel4.elf");
    let monitor_elf_path = elf_path.join("monitor.elf");
    let capdl_init_elf_path = elf_path.join("initialiser.elf");

    let kernel_config_path = sdk_dir
        .join("board")
        .join(args.board)
        .join(args.config)
        .join("include/kernel/gen_config.json");

    let invocations_all_path = sdk_dir
        .join("board")
        .join(args.board)
        .join(args.config)
        .join("invocations_all.json");

    if !elf_path.exists() {
        eprintln!(
            "Error: board ELF directory '{}' does not exist",
            elf_path.display()
        );
        std::process::exit(1);
    }
    if !kernel_elf_path.exists() {
        eprintln!(
            "Error: kernel ELF '{}' does not exist",
            kernel_elf_path.display()
        );
        std::process::exit(1);
    }
    if !monitor_elf_path.exists() {
        eprintln!(
            "Error: monitor ELF '{}' does not exist",
            monitor_elf_path.display()
        );
        std::process::exit(1);
    }
    if !capdl_init_elf_path.exists() {
        eprintln!(
            "Error: CapDL initialiser ELF '{}' does not exist",
            capdl_init_elf_path.display()
        );
        std::process::exit(1);
    }
    if !kernel_config_path.exists() {
        eprintln!(
            "Error: kernel configuration file '{}' does not exist",
            kernel_config_path.display()
        );
        std::process::exit(1);
    }
    if !invocations_all_path.exists() {
        eprintln!(
            "Error: invocations JSON file '{}' does not exist",
            invocations_all_path.display()
        );
        std::process::exit(1);
    }

    let system_path = Path::new(args.system);
    if !system_path.exists() {
        eprintln!(
            "Error: system description file '{}' does not exist",
            system_path.display()
        );
        std::process::exit(1);
    }

    let xml: String = fs::read_to_string(args.system).unwrap();

    let kernel_config_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(kernel_config_path).unwrap()).unwrap();

    let invocations_labels: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(invocations_all_path).unwrap()).unwrap();

    let arch = match json_str(&kernel_config_json, "SEL4_ARCH")? {
        "aarch64" => Arch::Aarch64,
        "riscv64" => Arch::Riscv64,
        "x86_64" => Arch::X86_64,
        _ => panic!("Unsupported kernel config architecture"),
    };

    let image_output_type = if let Some(image_type) = args.output_image_type {
        match ImageOutputType::parse(image_type, args.board, arch) {
            Ok(output_image_type) => output_image_type,
            Err(reason) => {
                eprintln!("microkit: error: argument --image-type: {reason}");
                std::process::exit(1);
            }
        }
    } else {
        ImageOutputType::default_from_arch(&arch)
    };

    let (device_regions, normal_regions) = match arch {
        Arch::X86_64 => (None, None),
        _ => {
            let kernel_platform_config_path = sdk_dir
                .join("board")
                .join(args.board)
                .join(args.config)
                .join("platform_gen.json");

            if !kernel_platform_config_path.exists() {
                eprintln!(
                    "Error: kernel platform configuration file '{}' does not exist",
                    kernel_platform_config_path.display()
                );
                std::process::exit(1);
            }

            let kernel_platform_config: PlatformConfig =
                serde_json::from_str(&fs::read_to_string(kernel_platform_config_path).unwrap())
                    .unwrap();

            (
                Some(kernel_platform_config.devices),
                Some(kernel_platform_config.memory),
            )
        }
    };

    let hypervisor = match arch {
        Arch::Aarch64 => json_str_as_bool(&kernel_config_json, "ARM_HYPERVISOR_SUPPORT")?,
        Arch::X86_64 => json_str_as_bool(&kernel_config_json, "VTX")?,
        // Hypervisor mode is not available on RISC-V
        _ => false,
    };

    let arm_pa_size_bits = match arch {
        Arch::Aarch64 => {
            if json_str_as_bool(&kernel_config_json, "ARM_PA_SIZE_BITS_40")? {
                Some(40)
            } else if json_str_as_bool(&kernel_config_json, "ARM_PA_SIZE_BITS_44")? {
                Some(44)
            } else {
                panic!("Expected ARM platform to have 40 or 44 physical address bits")
            }
        }
        Arch::X86_64 | Arch::Riscv64 => None,
    };

    let arm_smc = match arch {
        Arch::Aarch64 => Some(json_str_as_bool(&kernel_config_json, "ALLOW_SMC_CALLS")?),
        _ => None,
    };

    let x86_xsave_size = match arch {
        Arch::X86_64 => Some(json_str_as_u64(&kernel_config_json, "XSAVE_SIZE")? as usize),
        _ => None,
    };

    let kernel_frame_size = match arch {
        Arch::Aarch64 => 1 << 12,
        Arch::Riscv64 => 1 << 21,
        Arch::X86_64 => 1 << 12,
    };

    let kernel_config = Config {
        arch,
        word_size: json_str_as_u64(&kernel_config_json, "WORD_SIZE")?,
        minimum_page_size: 4096,
        paddr_user_device_top: json_str_as_u64(&kernel_config_json, "PADDR_USER_DEVICE_TOP")?,
        kernel_frame_size,
        init_cnode_bits: json_str_as_u64(&kernel_config_json, "ROOT_CNODE_SIZE_BITS")?,
        cap_address_bits: 64,
        fan_out_limit: json_str_as_u64(&kernel_config_json, "RETYPE_FAN_OUT_LIMIT")?,
        max_num_bootinfo_untypeds: json_str_as_u64(
            &kernel_config_json,
            "MAX_NUM_BOOTINFO_UNTYPED_CAPS",
        )?,
        hypervisor,
        benchmark: args.config == "benchmark",
        fpu: json_str_as_bool(&kernel_config_json, "HAVE_FPU")?,
        arm_pa_size_bits,
        arm_smc,
        riscv_pt_levels: Some(RiscvVirtualMemory::Sv39),
        x86_xsave_size,
        invocations_labels,
        device_regions,
        normal_regions,
    };

    if kernel_config.arch != Arch::X86_64 && !loader_elf_path.exists() {
        eprintln!(
            "Error: loader ELF '{}' does not exist",
            loader_elf_path.display()
        );
        std::process::exit(1);
    }

    if let Arch::Aarch64 = kernel_config.arch {
        assert!(
            kernel_config.hypervisor,
            "Microkit tool expects a kernel with hypervisor mode enabled on AArch64."
        );
    }

    assert!(
        kernel_config.word_size == 64,
        "Microkit tool has various assumptions about the word size being 64-bits."
    );

    let mut system = match parse(args.system, &xml, &kernel_config) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let capdl_initialiser_elf = ElfFile::from_path(&capdl_init_elf_path).unwrap();

    // Only relevant for ARM and RISC-V.
    // Determine how much physical memory is available to the kernel after it boots but before dropping
    // to userspace by partially emulating the kernel boot process. This is useful for two purposes:
    // 1. To implement setvar region_paddr for memory regions that doesn't specify a phys address, where
    //    we must automatically select a suitable address inside the Microkit tool.
    // 2. Post-spec generation sanity checks at a later point to ensure that there are sufficient memory
    //    to allocate all kernel objects.
    let (kernel_elf_maybe, available_memory_maybe, kernel_boot_region_maybe) =
        match kernel_config.arch {
            Arch::X86_64 => (None, None, None),
            Arch::Aarch64 | Arch::Riscv64 => {
                let kernel_elf = ElfFile::from_path(&kernel_elf_path).unwrap();

                // Now determine how much memory we have after the kernel boots.
                let (available_memory, kernel_boot_region) =
                    emulate_kernel_boot_partial(&kernel_config, &kernel_elf);
                (
                    Some(kernel_elf),
                    Some(available_memory),
                    Some(kernel_boot_region),
                )
            }
        };

    let monitor_elf = ElfFile::from_path(&monitor_elf_path)?;

    let mut search_paths = vec![std::env::current_dir().unwrap()];
    for path in args.search_paths {
        search_paths.push(PathBuf::from(path));
    }

    // This list refers to all PD ELFs as well as the Monitor ELF.
    // The monitor is very similar to a PD so it is useful to pass around
    // a list like this.
    let mut system_elfs = Vec::with_capacity(system.protection_domains.len());
    // Get the elf files for each pd:
    for pd in &system.protection_domains {
        match get_full_path(&pd.program_image, &search_paths) {
            Some(path) => {
                system_elfs.push(ElfFile::from_path(&path)?);
            }
            None => {
                return Err(format!(
                    "unable to find program image: '{}'",
                    pd.program_image.display()
                ))
            }
        }
    }

    // The monitor is just a special PD
    system_elfs.push(monitor_elf);

    let mut capdl_initialiser = CapDLInitialiser::new(capdl_initialiser_elf);

    // Now build the capDL spec and final image. We may need to do this in >1 iterations on ARM and RISC-V
    // if there are Memory Regions without a paddr but subject to setvar region_paddr.
    let mut iteration = 0;
    let mut spec_need_refinement = true;
    let mut system_built = false;
    while spec_need_refinement && iteration < MAX_BUILD_ITERATION {
        spec_need_refinement = false;

        // Patch all the required symbols in the Monitor and PDs according to the Microkit's requirements
        if let Err(err) = patch_symbols(&kernel_config, &mut system_elfs, &system) {
            eprintln!("ERROR: {err}");
            std::process::exit(1);
        }

        let mut spec_container = build_capdl_spec(&kernel_config, &mut system_elfs, &system)?;
        pack_spec_into_initial_task(
            &kernel_config,
            args.config,
            &spec_container,
            &system_elfs,
            &mut capdl_initialiser,
        );

        match kernel_config.arch {
            Arch::X86_64 => {
                // setvar region_paddr not supported on this architecture nor can we emulate the
                // kernel boot process to statically check for issues due to unknown memory map, so nothing to do.
                // Write out the capDL initialiser as an ELF boot module and we are done.
            }
            Arch::Aarch64 | Arch::Riscv64 => {
                // Now that we have the CapDL initialiser ELF with embedded spec,
                // we can determine exactly how much memory will be available statically when the kernel
                // drops to userspace on ARM and RISC-V. This allow us to sanity check that:
                // 1. There are enough memory to allocate all the objects required in the spec.
                // 2. All frames with a physical attached reside in legal memory (device or normal).
                // 3. Objects can be allocated from the free untyped list. For example, we detect
                //    situations where you might have a few frames with size bit 12 to allocate but
                //    only have untyped with size bit <12 remaining.
                // This also allow the tool to automatically pick physical address of Memory Regions with out
                // an explicit paddr in SDF but are subject to setvar region_paddr.

                // Determine how much memory the CapDL initialiser needs.
                let initialiser_vaddr_range = capdl_initialiser.image_bound();
                let initial_task_size = initialiser_vaddr_range.end - initialiser_vaddr_range.start;

                // Reuse data from the partial kernel boot emulation previously done.
                // .clone() as we need to mutate this for every iteration.
                let mut available_memory = available_memory_maybe.clone().unwrap();
                let kernel_boot_region = kernel_boot_region_maybe.unwrap();

                // The kernel relies on the initial task region being allocated above the kernel
                // boot/ELF region, so we have the end of the kernel boot region as the lower
                // bound for allocating the reserved region.
                let initial_task_phys_base_maybe =
                    available_memory.allocate_from(initial_task_size, kernel_boot_region.end);
                if initial_task_phys_base_maybe.is_none() {
                    // Unlikely to happen on Microkit-supported platforms with multi gigabytes memory.
                    // But printing a helpful error in case we do run into this problem.
                    eprintln!(
                        "ERROR: cannot allocate memory for the initialiser, contiguous physical memory region of size {} not found", human_size_strict(initial_task_size)
                    );
                    eprintln!("ERROR: physical memory regions the initialiser can be placed at:");
                    for region in available_memory.regions {
                        eprintln!(
                            "       [0x{:0>12x}..0x{:0>12x}), size: {}",
                            region.base,
                            region.end,
                            human_size_strict(region.size())
                        );
                    }
                    std::process::exit(1);
                }

                let initial_task_phys_base = initial_task_phys_base_maybe.unwrap();
                capdl_initialiser.set_phys_base(initial_task_phys_base);
                let initial_task_phys_region = MemoryRegion::new(
                    initial_task_phys_base,
                    initial_task_phys_base + initial_task_size,
                );
                let user_image_virt_region = MemoryRegion::new(
                    capdl_initialiser.elf.lowest_vaddr(),
                    initialiser_vaddr_range.end,
                );

                // With the initial task region determined the kernel boot can be emulated in full. This provides
                // the boot info information (containing untyped objects) which is needed for the next steps
                let kernel_boot_info = emulate_kernel_boot(
                    &kernel_config,
                    kernel_elf_maybe.as_ref().unwrap(),
                    initial_task_phys_region,
                    user_image_virt_region,
                );

                if iteration == 0 {
                    // On the first iteration where the spec have not been refined, simulate the capDL allocation algorithm
                    // to double check that all kernel objects of the system as described by SDF can be successfully allocated.
                    if !simulate_capdl_object_alloc_algorithm(
                        &mut spec_container,
                        &kernel_boot_info,
                        &kernel_config,
                        CapDLAllocEmulationErrorLevel::PrintStderr,
                    ) {
                        eprintln!("ERROR: could not allocate all required kernel objects. Please see report for more details.");
                        std::process::exit(1);
                    }
                } else {
                    // Do the same thing for further iterations, at this point the simulation won't fail *except* for when we have picked a
                    // bad address for Memory Regions subject to setvar region_paddr. This can happen because after we have
                    // picked the address, we will update spec and patch it into the program's frame. Which will causes the
                    // spec to increase in size as the frames' data are compressed. So if the simulation fail, we need to
                    // pick another address as we now have a better idea of how large the spec is.

                    // This is highly unlikely to happen unless the spec size increase causes the initial task size to cross
                    // a 4K page boundary.
                    if !simulate_capdl_object_alloc_algorithm(
                        &mut spec_container,
                        &kernel_boot_info,
                        &kernel_config,
                        CapDLAllocEmulationErrorLevel::Suppressed,
                    ) {
                        // Encountered a problem, pick a better address.
                        for tool_allocate_mr in system.memory_regions.iter_mut().filter(|mr| {
                            matches!(mr.phys_addr, SysMemoryRegionPaddr::ToolAllocated(_))
                        }) {
                            tool_allocate_mr.phys_addr = SysMemoryRegionPaddr::ToolAllocated(None);
                        }
                        spec_container.expected_allocations = HashMap::new();
                    }
                }

                // Now pick a physical address for any memory regions that are subject to setvar region_paddr.
                // Doing something a bit unconventional here: converting the list of untypeds back to a DisjointMemoryRegion
                // to give us a view of physical memory available after the kernel drops to user space.
                // I.e. available memory after the initial task have been created.
                {
                    let mut available_user_memory = DisjointMemoryRegion::default();
                    for ut in kernel_boot_info
                        .untyped_objects
                        .iter()
                        .filter(|ut| !ut.is_device)
                    {
                        // Only take untypeds that can at least fit a page because some have been used to back the initial task's
                        // kernel object such as TCB, endpoint etc.
                        let start = round_up(ut.base(), kernel_config.minimum_page_size);
                        let end = round_down(ut.end(), kernel_config.minimum_page_size);
                        if end > start {
                            // will be automatically merged
                            available_user_memory.insert_region(ut.base(), ut.end());
                        }
                    }

                    // Then take away any memory ranges occupied by Memory Regions with a paddr specified in SDF.
                    for mr in system.memory_regions.iter() {
                        if let SysMemoryRegionPaddr::Specified(sdf_paddr) = mr.phys_addr {
                            let mr_end = sdf_paddr + mr.size;

                            // MR may be device memory, which isn't covered in available_user_memory.
                            let is_normal_mem =
                                available_user_memory.regions.iter().any(|region| {
                                    sdf_paddr >= region.base
                                        && sdf_paddr < region.end
                                        && mr_end <= region.end
                                });
                            if is_normal_mem {
                                available_user_memory.remove_region(sdf_paddr, sdf_paddr + mr.size);
                            }
                        }
                    }

                    let mut tool_allocated_mrs = Vec::new();
                    for (mr_id, tool_allocate_mr) in system
                        .memory_regions
                        .iter_mut()
                        .enumerate()
                        .filter(|(_, mr)| {
                            matches!(mr.phys_addr, SysMemoryRegionPaddr::ToolAllocated(None))
                        })
                    {
                        spec_need_refinement = true;

                        let target_paddr = available_user_memory
                            .allocate(tool_allocate_mr.size, tool_allocate_mr.page_size);
                        if target_paddr.is_none() {
                            eprintln!("ERROR: cannot auto-select a physical address for MR {} because there are no contiguous memory region of sufficient size.", tool_allocate_mr.name);
                            eprintln!("ERROR: MR {} needs to be physically contiguous as it is a subject of a setvar region_paddr.", tool_allocate_mr.name);
                            if !tool_allocated_mrs.is_empty() {
                                eprintln!("Previously auto-allocated memory regions:");
                                for allocated_mr_id in tool_allocated_mrs {
                                    let allocated_mr: &SysMemoryRegion =
                                        &system.memory_regions[allocated_mr_id];
                                    eprintln!(
                                        "name = '{}', paddr = 0x{:0>12x}, size = 0x{:0>12x}",
                                        allocated_mr.name,
                                        allocated_mr.paddr().unwrap(),
                                        allocated_mr.size
                                    );
                                }
                            }
                            eprintln!("available physical memory regions:");
                            for region in available_user_memory.regions {
                                eprintln!(
                                    "[0x{:0>12x}..0x{:0>12x}), size: {}",
                                    region.base,
                                    region.end,
                                    human_size_strict(region.size())
                                );
                            }
                            std::process::exit(1);
                        }
                        tool_allocated_mrs.push(mr_id);
                        tool_allocate_mr.phys_addr =
                            SysMemoryRegionPaddr::ToolAllocated(target_paddr);
                    }
                }

                // Patch the list of untypeds we used to simulate object allocation into the initialiser.
                // At runtime the intialiser will validate what we simulated against what the kernel gives it. If they deviate
                // we will have problems! For example, if we simulated with more memory than what's actually available, the initialiser
                // can crash.
                capdl_initialiser.add_expected_untypeds(&kernel_boot_info.untyped_objects);
            }
        };

        if !spec_need_refinement {
            // All is well in the universe, write the image out.
            println!(
                "MICROKIT|CAPDL SPEC: number of root objects = {}, spec footprint = {}",
                spec_container.spec.objects.len(),
                human_size_strict(
                    capdl_initialiser
                        .spec_metadata()
                        .as_ref()
                        .unwrap()
                        .spec_size
                ),
            );
            let initialiser_vaddr_range = capdl_initialiser.image_bound();
            println!(
                "MICROKIT|INITIAL TASK: memory size = {}",
                human_size_strict(initialiser_vaddr_range.end - initialiser_vaddr_range.start),
            );

            let image_out_path = Path::new(args.output);

            match kernel_config.arch {
                Arch::X86_64 => match capdl_initialiser.elf.reserialise(image_out_path) {
                    Ok(size) => {
                        // Copy the kernel to the build directory as well so users doesn't have to dig through the SDK.
                        if let Err(copy_err) = fs::copy(
                            &kernel_elf_path,
                            image_out_path.parent().unwrap().join(KERNEL_COPY_FILENAME),
                        ) {
                            eprintln!("ERROR: couldn't copy the kernel to image's output directory: {copy_err}");
                            std::process::exit(1);
                        }
                        if let Err(copy_err) = fs::copy(
                            kernel_elf_path
                                .parent()
                                .unwrap()
                                .join(KERNEL32_COPY_FILENAME),
                            image_out_path
                                .parent()
                                .unwrap()
                                .join(KERNEL32_COPY_FILENAME),
                        ) {
                            eprintln!("ERROR: couldn't copy the 32-bit kernel to image's output directory: {copy_err}");
                            std::process::exit(1);
                        }
                        println!(
                            "MICROKIT|BOOT MODULE: image file size = {}",
                            human_size_strict(size)
                        );
                    }
                    Err(err) => {
                        eprintln!("ERROR: couldn't write the boot module to filesystem: {err}");
                        std::process::exit(1);
                    }
                },
                Arch::Aarch64 | Arch::Riscv64 => {
                    let loader = Loader::new(
                        &kernel_config,
                        Path::new(&loader_elf_path),
                        kernel_elf_maybe.as_ref().unwrap(),
                        &capdl_initialiser.elf,
                        capdl_initialiser.phys_base.unwrap(),
                        &initialiser_vaddr_range,
                    );

                    match image_output_type {
                        ImageOutputType::Binary => loader.write_image(image_out_path),
                        ImageOutputType::Elf => loader.write_elf(image_out_path),
                        ImageOutputType::Uimage => loader.write_uimage(image_out_path),
                    };

                    println!(
                        "MICROKIT|LOADER: image file size = {}",
                        human_size_strict(metadata(image_out_path).unwrap().len())
                    );
                }
            };

            if let Some(capdl_json) = args.capdl_json {
                let serialised = serde_json::to_string_pretty(&spec_container.spec).unwrap();
                fs::write(capdl_json, &serialised).unwrap();
            };

            write_report(&spec_container, &kernel_config, args.report);
            system_built = true;
            break;
        } else {
            // Some memory regions have had their physical address updated, rebuild the spec.
            iteration += 1;
        }
    }

    if !system_built {
        // Cannot build a reasonable spec, absurd.
        // Only reachable when there are setvar region_paddr that we keep selecting the wrong address.
        panic!("ERROR: fatal, failed to build system in {iteration} iterations");
    }

    Ok(())
}
