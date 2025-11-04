//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use microkit_tool::capdl::allocation::simulate_capdl_object_alloc_algorithm;
use microkit_tool::capdl::initialiser::{CapDLInitialiser, DEFAULT_INITIALISER_HEAP_MULTIPLIER};
use microkit_tool::capdl::spec::ElfContent;
use microkit_tool::capdl::{build_capdl_spec, reserialise_spec};
use microkit_tool::elf::ElfFile;
use microkit_tool::loader::Loader;
use microkit_tool::report::write_report;
use microkit_tool::sdf::parse;
use microkit_tool::sel4::{
    emulate_kernel_boot, emulate_kernel_boot_partial, Arch, Config, PlatformConfig,
    RiscvVirtualMemory,
};
use microkit_tool::symbols::patch_symbols;
use microkit_tool::util::{human_size_strict, json_str, json_str_as_bool, json_str_as_u64};
use microkit_tool::MemoryRegion;
use sel4_capdl_initializer_types::{ObjectNamesLevel, Spec};
use std::fs::{self, metadata};
use std::path::{Path, PathBuf};

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
    println!("usage: microkit [-h] [-o OUTPUT] [-r REPORT] --board BOARD --config CONFIG [--capdl-spec CAPDL_SPEC --search-path [SEARCH_PATH ...]] system")
}

fn print_help(available_boards: &[String]) {
    print_usage();
    println!("\npositional arguments:");
    println!("  system");
    println!("\noptions:");
    println!("  -h, --help, show this help message and exit");
    println!("  -o, --output OUTPUT");
    println!("  -r, --report REPORT");
    println!("  --board {}", available_boards.join("\n          "));
    println!("  --config CONFIG");
    println!("  --capdl-spec CAPDL_SPEC (outputs as JSON)");
    println!("  --search-path [SEARCH_PATH ...]");
}

struct Args<'a> {
    system: &'a str,
    board: &'a str,
    config: &'a str,
    report: &'a str,
    capdl_spec: Option<&'a str>,
    output: &'a str,
    search_paths: Vec<&'a String>,
    initialiser_heap_size_multiplier: f64,
}

impl<'a> Args<'a> {
    pub fn parse(args: &'a [String], available_boards: &[String]) -> Args<'a> {
        // Default arguments
        let mut output = "loader.img";
        let mut report = "report.txt";
        let mut capdl_spec = None;
        let mut search_paths = Vec::new();
        // Arguments expected to be provided by the user
        let mut system = None;
        let mut board = None;
        let mut config = None;
        let mut initialiser_heap_size_multiplier = DEFAULT_INITIALISER_HEAP_MULTIPLIER;

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
                "--capdl-spec" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        capdl_spec = Some(args[i + 1].as_str());
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --capdl-spec: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--initialiser_heap_size_multiplier" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        match args[i + 1].parse::<f64>() {
                            Ok(multiplier) => initialiser_heap_size_multiplier = multiplier,
                            Err(e) => {
                                eprintln!("microkit: error: argument --initialiser_heap_size_multiplier: failed to parse as float: {e}");
                                std::process::exit(1);
                            }
                        }
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --initialiser_heap_size_multiplier: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--search-path" => {
                    in_search_path = true;
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
            capdl_spec,
            output,
            search_paths,
            initialiser_heap_size_multiplier,
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

    let system = match parse(args.system, &xml, &kernel_config) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    // Only relevant for ARM and RISC-V.
    // Determine how much physical memory is available to the kernel after it boots but before dropping
    // to userspace by partially emulating the kernel boot process. This is useful for two purposes:
    // 1. To implement setvar region_paddr for memory regions that doesn't specify a phys address, where
    //    we can automatically select a suitable address inside the Microkit tool.
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

    let mut monitor_elf = ElfFile::from_path(&monitor_elf_path)?;

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

    // Patch all the required symbols in the Monitor and PDs according to the Microkit's requirements
    if let Err(err) = patch_symbols(&kernel_config, &mut system_elfs, &mut monitor_elf, &system) {
        eprintln!("ERROR: {err}");
        std::process::exit(1);
    }

    // The monitor is just a special PD
    system_elfs.push(monitor_elf);
    // We have parsed the XML and all ELF files, create the CapDL spec of the system described in the XML.
    let mut spec = build_capdl_spec(&kernel_config, &mut system_elfs, &system)?;

    // Reserialise the spec into a type that can be understood by rust-sel4.
    let spec_reserialised = {
        // Eagerly write out the spec so we can debug in case something crash later.
        let spec_as_json = if args.capdl_spec.is_some() {
            let serialised = serde_json::to_string_pretty(&spec).unwrap();
            fs::write(args.capdl_spec.unwrap(), &serialised).unwrap();
            serialised
        } else {
            serde_json::to_string(&spec).unwrap()
        };

        // The full type definition is `Spec<'a, N, D, M>` where:
        // N = object name type
        // D = frame fill data type
        // M = embedded frame data type
        // Only N and D is useful for Microkit.
        serde_json::from_str::<Spec<String, ElfContent, ()>>(&spec_as_json).unwrap()
    };

    // Now embed the built spec into the CapDL initialiser.
    let name_level = match args.config {
        "debug" => ObjectNamesLevel::All,
        // We don't copy over the object names as there is no debug printing in these configuration to save memory.
        "release" | "benchmark" => ObjectNamesLevel::None,
        _ => panic!("unknown configuration {}", args.config),
    };

    let num_objects = spec.objects.len();
    let capdl_spec_as_binary =
        reserialise_spec::reserialise_spec(&system_elfs, &spec_reserialised, &name_level);

    // Patch the spec and heap into the ELF image.
    let mut capdl_initialiser = CapDLInitialiser::new(
        ElfFile::from_path(&capdl_init_elf_path)?,
        args.initialiser_heap_size_multiplier,
    );
    capdl_initialiser.add_spec(capdl_spec_as_binary);

    println!(
        "MICROKIT|CAPDL SPEC: number of root objects = {}, spec footprint = {}, initialiser heap size = {}",
        num_objects,
        human_size_strict(capdl_initialiser.spec_size.unwrap()),
        human_size_strict(capdl_initialiser.heap_size.unwrap())
    );
    let initialiser_vaddr_range = capdl_initialiser.image_bound();
    println!(
        "MICROKIT|INITIAL TASK: memory size = {}",
        human_size_strict(initialiser_vaddr_range.end - initialiser_vaddr_range.start),
    );

    // For x86 we write out the initialiser ELF as is, but on ARM and RISC-V we build the bootloader image.
    match kernel_config.arch {
        Arch::X86_64 => match capdl_initialiser.elf.reserialise(Path::new(args.output)) {
            Ok(size) => {
                println!(
                    "MICROKIT|BOOT MODULE: image file size = {}",
                    human_size_strict(size)
                );
            }
            Err(err) => {
                eprintln!("Error: couldn't write the boot module to filesystem: {err}");
            }
        },
        Arch::Aarch64 | Arch::Riscv64 => {
            // Now that we have the entire spec and CapDL initialiser ELF with embedded spec,
            // we can determine exactly how much memory will be available statically when the kernel
            // drops to userspace on ARM and RISC-V. This allow us to sanity check that:
            // 1. There are enough memory to allocate all the objects required in the spec.
            // 2. All frames with a physical attached reside in legal memory (device or normal).
            // 3. Objects can be allocated from the free untyped list. For example, we detect
            //    situations where you might have a few frames with size bit 12 to allocate but
            //    only have untyped with size bit <12 remaining.

            // Determine how much memory the CapDL initialiser needs.
            let initial_task_size = initialiser_vaddr_range.end - initialiser_vaddr_range.start;

            // Reuse data from the partial kernel boot emulation previously done.
            let kernel_elf = kernel_elf_maybe.unwrap();
            let mut available_memory = available_memory_maybe.unwrap();
            let kernel_boot_region = kernel_boot_region_maybe.unwrap();

            // The kernel relies on the initial task region being allocated above the kernel
            // boot/ELF region, so we have the end of the kernel boot region as the lower
            // bound for allocating the reserved region.
            let initial_task_phys_base_maybe =
                available_memory.allocate_from(initial_task_size, kernel_boot_region.end);
            if initial_task_phys_base_maybe.is_none() {
                // Unlikely to happen on Microkit-supported platforms with multi gigabytes memory.
                // But printing a helpful error in case we do run into this problem.
                eprintln!("Error: out of contiguous physical memory to place the initial task.");
                eprintln!("Help: initial task size is: {}", human_size_strict(initial_task_size));
                eprintln!("Help: physical memory regions on this platform after kernel boot is:");
                for region in available_memory.regions {
                    eprintln!(
                        "Help: [0x{:0>12x}..0x{:0>12x}), size: {}",
                        region.base,
                        region.end,
                        human_size_strict(region.size())
                    );
                }
                std::process::exit(1);
            }

            let initial_task_phys_base = initial_task_phys_base_maybe.unwrap();
            let initial_task_phys_region = MemoryRegion::new(
                initial_task_phys_base,
                initial_task_phys_base + initial_task_size,
            );
            let initial_task_virt_region = MemoryRegion::new(
                capdl_initialiser.elf.lowest_vaddr(),
                initialiser_vaddr_range.end,
            );

            // With the initial task region determined the kernel boot can be emulated in full. This provides
            // the boot info information which is needed for the next steps
            let kernel_boot_info = emulate_kernel_boot(
                &kernel_config,
                &kernel_elf,
                initial_task_phys_region,
                initial_task_virt_region,
            );

            let alloc_ok =
                simulate_capdl_object_alloc_algorithm(&mut spec, &kernel_boot_info, &kernel_config);
            write_report(&spec, &kernel_config, args.report);
            if !alloc_ok {
                eprintln!("ERROR: could not allocate all required kernel objects. Please see report for more details.");
                std::process::exit(1);
            }

            // Everything checks out, patch the list of untypeds we used to simulate object allocation into the initialiser.
            // At runtime the intialiser will validate what we simulated against what the kernel gives it. If they deviate
            // we will have problems! For example, if we simulated with more memory than what's actually available, the initialiser
            // can crash.
            capdl_initialiser.add_expected_untypeds(&kernel_boot_info.untyped_objects);

            // Everything checks out, now build the bootloader!
            let loader = Loader::new(
                &kernel_config,
                Path::new(&loader_elf_path),
                &kernel_elf,
                &capdl_initialiser.elf,
                initial_task_phys_base,
                initialiser_vaddr_range,
            );

            loader.write_image(Path::new(args.output));

            println!(
                "MICROKIT|LOADER: image file size = {}",
                human_size_strict(metadata(args.output).unwrap().len())
            );
        }
    };

    write_report(&spec, &kernel_config, args.report);

    Ok(())
}
