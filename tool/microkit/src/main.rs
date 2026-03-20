//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use microkit_tool::argparse;
use microkit_tool::argparse::{Args, ArgsError, RequestedImageType};
use microkit_tool::capdl::allocation::{
    simulate_capdl_object_alloc_algorithm, CapDLAllocEmulationErrorLevel,
};
use microkit_tool::capdl::build_capdl_spec;
use microkit_tool::capdl::initialiser::CapDLInitialiser;
use microkit_tool::capdl::packaging::pack_spec_into_initial_task;
use microkit_tool::elf::ElfFile;
use microkit_tool::jsonparse;
use microkit_tool::loader::Loader;
use microkit_tool::report::write_report;
use microkit_tool::sdf::{parse, SysMemoryRegion, SysMemoryRegionPaddr};
use microkit_tool::sdk::Sdk;
use microkit_tool::sel4::{
    emulate_kernel_boot, emulate_kernel_boot_partial, Arch, Config, RiscvVirtualMemory,
};
use microkit_tool::symbols::patch_symbols;
use microkit_tool::util::{get_full_path, human_size_strict, round_down, round_up};
use microkit_tool::{DisjointMemoryRegion, MemoryRegion};

use std::collections::HashMap;
use std::fmt;
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

enum ImageOutputType {
    Binary,
    Elf,
    Uimage,
}

impl ImageOutputType {
    fn default_from_arch_and_board(arch: &Arch, board_name: &str) -> Self {
        match board_name {
            "ariane" | "cheshire" | "serengeti" => ImageOutputType::Elf,
            _ => match arch {
                Arch::Aarch64 => ImageOutputType::Binary,
                Arch::Riscv64 => ImageOutputType::Uimage,
                Arch::X86_64 => ImageOutputType::Elf,
            },
        }
    }

    /// Resolve the optional user-specified image type with what is the default for the
    /// platform.
    /// Not all image types are supported for all platforms, so we check here.
    fn resolve(requested: &RequestedImageType, arch: &Arch, board_name: &str) -> Option<Self> {
        match requested {
            RequestedImageType::Binary => match arch {
                Arch::Aarch64 | Arch::Riscv64 => Some(Self::Binary),
                Arch::X86_64 => None,
            },
            RequestedImageType::Elf => Some(Self::Elf),
            RequestedImageType::Uimage => match arch {
                Arch::Riscv64 => Some(Self::Uimage),
                Arch::X86_64 | Arch::Aarch64 => None,
            },
            RequestedImageType::Unspecified => {
                Some(Self::default_from_arch_and_board(arch, board_name))
            }
        }
    }
}

enum KernelBootType {
    // The boot type used for x86_64 systems:
    // setvar region_paddr not supported on this architecture nor can we emulate the
    // kernel boot process to statically check for issues due to unknown memory map, so nothing to do.
    // Write out the capDL initialiser as an ELF boot module and we are done.
    X86_64,

    // The boot type used for ARM and RISC-V:
    // Determine how much physical memory is available to the kernel after it boots but before dropping
    // to userspace by partially emulating the kernel boot process. This is useful for two purposes:
    // 1. To implement setvar region_paddr for memory regions that doesn't specify a phys address, where
    //    we must automatically select a suitable address inside the Microkit tool.
    // 2. Post-spec generation sanity checks at a later point to ensure that we have sufficient memory
    //    to allocate all kernel objects.
    Static {
        kernel_elf: ElfFile,
        available_memory: DisjointMemoryRegion,
        kernel_boot_region: MemoryRegion,
    }
}

enum MainError {
    MissingPath {
        description: &'static str,
        path: PathBuf,
    },
    JsonError {
        source: jsonparse::JsonError,
    },
    UnsupportedKernelArch {
        value: String,
    },
    UnsupportedImageType {
        requested: RequestedImageType,
        arch: Arch,
    },
    MissingArmPaSizeBits,
    Aarch64HypervisorRequired,
    UnsupportedWordSize {
        word_size: u64,
    },
}

impl fmt::Display for MainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPath { description, path } => {
                write!(f, "{description} '{}' does not exist", path.display())
            }
            Self::JsonError { source } => {
                write!(f, "{source}")
            }
            Self::UnsupportedKernelArch { value } => {
                write!(f, "unsupported kernel config architecture '{value}'")
            }
            Self::UnsupportedImageType { requested, arch } => {
                write!(
                    f,
                    "building the output image as '{}' is unsupported for target architecture '{}'",
                    requested, arch
                )
            }
            Self::MissingArmPaSizeBits => {
                write!(
                    f,
                    "expected ARM platform to have 40 or 44 physical address bits"
                )
            }
            Self::Aarch64HypervisorRequired => {
                write!(f, "Microkit requires hypervisor mode on AArch64")
            }
            Self::UnsupportedWordSize { word_size } => {
                write!(f, "Microkit requires 64-bit word size, found {}", word_size)
            }
        }
    }
}

impl From<jsonparse::JsonError> for MainError {
    fn from(source: jsonparse::JsonError) -> Self {
        MainError::JsonError { source }
    }
}

fn bail_if_not_exists(description: &'static str, path: &Path) -> Result<(), MainError> {
    if !path.exists() {
        Err(MainError::MissingPath {
            description,
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn build_kernel_config(
    args: &Args,
    config_dir: &Path,
    kernel_config_path: &Path,
    invocations_all_path: &Path,
) -> Result<Config, MainError> {
    let kernel_config_json = jsonparse::read("kernel configuration file", kernel_config_path)?;
    let invocations_labels = jsonparse::read("invocations JSON file", invocations_all_path)?;

    let arch = match kernel_config_json.string("SEL4_ARCH")? {
        "aarch64" => Arch::Aarch64,
        "riscv64" => Arch::Riscv64,
        "x86_64" => Arch::X86_64,
        value => {
            return Err(MainError::UnsupportedKernelArch {
                value: value.to_owned(),
            });
        }
    };

    let (device_regions, normal_regions) = match arch {
        Arch::X86_64 => (None, None),
        _ => {
            let platform_gen_path = config_dir.join("platform_gen.json");
            bail_if_not_exists("kernel platform configuration file", &platform_gen_path)?;

            let platform = jsonparse::read_platform_config(
                "kernel platform configuration file",
                &platform_gen_path,
            )?;

            (Some(platform.devices), Some(platform.memory))
        }
    };

    let hypervisor = match arch {
        Arch::Aarch64 => kernel_config_json.bool("ARM_HYPERVISOR_SUPPORT")?,
        Arch::X86_64 => kernel_config_json.bool("VTX")?,
        Arch::Riscv64 => false,
    };

    let arm_pa_size_bits = match arch {
        Arch::Aarch64 => {
            if kernel_config_json.bool("ARM_PA_SIZE_BITS_40")? {
                Some(40)
            } else if kernel_config_json.bool("ARM_PA_SIZE_BITS_44")? {
                Some(44)
            } else {
                return Err(MainError::MissingArmPaSizeBits);
            }
        }
        Arch::X86_64 | Arch::Riscv64 => None,
    };

    let object_sizes = {
        let object_sizes_path = config_dir.join("platform_gen.json");
        bail_if_not_exists("object sizes file", &object_sizes_path)?;

        serde_json::from_str(&fs::read_to_string(object_sizes_path).unwrap()).unwrap()
    };

    let arm_smc = match arch {
        Arch::Aarch64 => Some(kernel_config_json.bool("ALLOW_SMC_CALLS")?),
        _ => None,
    };

    let kernel_frame_size = match arch {
        Arch::Aarch64 => 1 << 12,
        Arch::Riscv64 => 1 << 21,
        Arch::X86_64 => 1 << 12,
    };

    let kernel_config = Config {
        arch,
        word_size: kernel_config_json.u64("WORD_SIZE")?,
        minimum_page_size: 4096,
        paddr_user_device_top: kernel_config_json.u64("PADDR_USER_DEVICE_TOP")?,
        kernel_frame_size,
        init_cnode_bits: kernel_config_json.u64("ROOT_CNODE_SIZE_BITS")?,
        cap_address_bits: 64,
        fan_out_limit: kernel_config_json.u64("RETYPE_FAN_OUT_LIMIT")?,
        max_num_bootinfo_untypeds: kernel_config_json.u64("MAX_NUM_BOOTINFO_UNTYPED_CAPS")?,
        hypervisor,
        benchmark: args.config == "benchmark" || args.config == "smp-benchmark",
        num_cores: if kernel_config_json.bool("ENABLE_SMP_SUPPORT")? {
            kernel_config_json.u8("MAX_NUM_NODES")?
        } else {
            1
        },
        fpu: kernel_config_json.bool("HAVE_FPU")?,
        arm_pa_size_bits,
        arm_smc,
        riscv_pt_levels: Some(RiscvVirtualMemory::Sv39),
        invocations_labels: invocations_labels.value,
        object_sizes,
        device_regions,
        normal_regions,
    };

    if let Arch::Aarch64 = kernel_config.arch {
        if !kernel_config.hypervisor {
            return Err(MainError::Aarch64HypervisorRequired);
        }
    }

    if kernel_config.word_size != 64 {
        return Err(MainError::UnsupportedWordSize {
            word_size: kernel_config.word_size,
        });
    }

    Ok(kernel_config)
}

fn main() -> Result<(), String> {
    let sdk = match Sdk::discover() {
        Ok(discovered_info) => discovered_info,
        Err(err) => {
            argparse::print_usage();
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    };

    let env_args: Vec<_> = std::env::args().collect();
    let args = match Args::parse(&env_args, &sdk) {
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

    // NB safe unwrap: argparse would already have bailed if the config did not
    // exist.
    let current_config = sdk.select(&args.board, &args.config).unwrap();

    // the real work begins here
    let elf_path = current_config.config_dir.join("elf");
    let loader_elf_path = elf_path.join("loader.elf");
    let kernel_elf_path = match args.override_kernel {
        Some(ref path) => path,
        None => &elf_path.join("sel4.elf"),
    };
    let monitor_elf_path = elf_path.join("monitor.elf");
    let capdl_init_elf_path = elf_path.join("initialiser.elf");
    let kernel_config_path = current_config
        .config_dir
        .join("include/kernel/gen_config.json");
    let invocations_all_path = current_config.config_dir.join("invocations_all.json");
    // bail_if_not_exists("board ELF directory", &elf_path)?;
    // bail_if_not_exists("kernel ELF", &kernel_elf_path)?;
    // bail_if_not_exists("monitor ELF", &monitor_elf_path)?;
    // bail_if_not_exists("CapDL initialiser ELF", &capdl_init_elf_path)?;
    // bail_if_not_exists("kernel configuration file", &kernel_config_path)?;
    // bail_if_not_exists("invocations JSON file", &invocations_all_path)?;

    let kernel_config = match build_kernel_config(
        &args,
        current_config.config_dir.as_path(),
        &kernel_config_path,
        &invocations_all_path,
    ) {
        Ok(kernel_config) => kernel_config,
        Err(err) => {
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    };

    let image_output_type = match ImageOutputType::resolve(
        &args.requested_image_type,
        &kernel_config.arch,
        args.board.as_str(),
    ) {
        Some(image) => image,
        None => {
            let err = MainError::UnsupportedImageType {
                requested: args.requested_image_type.clone(),
                arch: kernel_config.arch,
            };
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    };

    if kernel_config.arch != Arch::X86_64 {
        if let Err(err) = bail_if_not_exists("loader ELF", &loader_elf_path) {
            eprintln!("microkit: error: {err}");
            std::process::exit(1);
        }
    }

    let system_path = &args.sdf_path;
    // bail_if_not_exists("system description file", &system_path)?;
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

    let capdl_initialiser_elf = ElfFile::from_path(&capdl_init_elf_path).unwrap_or_else(|e| {
        eprintln!(
            "ERROR: failed to parse initialiser ELF ({}): {}",
            capdl_init_elf_path.display(),
            e
        );
        std::process::exit(1);
    });

    let kernel_boot_type: KernelBootType = match kernel_config.arch {
        Arch::X86_64 => { KernelBootType::X86_64 }
        Arch::Aarch64 | Arch::Riscv64 => {
            let kernel_elf = ElfFile::from_path(kernel_elf_path).unwrap_or_else(|e| {
                eprintln!(
                    "ERROR: failed to parse kernel ELF ({}): {}",
                    kernel_elf_path.display(),
                    e
                );
                std::process::exit(1);
            }); // TODO: improve error handling here

            // Now determine how much memory we have after the kernel boots.
            let (available_memory, kernel_boot_region) =
                emulate_kernel_boot_partial(&kernel_config, &kernel_elf);
            KernelBootType::Static {
                kernel_elf,
                available_memory,
                kernel_boot_region,
            }
        }
    };

    let monitor_elf = ElfFile::from_path(&monitor_elf_path).unwrap_or_else(|e| {
        eprintln!(
            "ERROR: failed to parse monitor ELF ({}): {}",
            monitor_elf_path.display(),
            e
        );
        std::process::exit(1);
    });

    // This list refers to all PD ELFs as well as the Monitor ELF.
    // The monitor is very similar to a PD so it is useful to pass around
    // a list like this.
    let mut system_elfs = Vec::with_capacity(system.protection_domains.len());
    // Get the elf files for each pd:
    for pd in &system.protection_domains {
        match get_full_path(&pd.program_image, &args.search_paths) {
            Some(path) => {
                let path_for_symbols = pd
                    .program_image_for_symbols
                    .as_ref()
                    .map(|path_suffix| {
                        get_full_path(path_suffix, &args.search_paths).ok_or_else(|| {
                            format!(
                                "unable to find program image for symbols: '{}'",
                                path_suffix.display()
                            )
                        })
                    })
                    .transpose()?;
                match ElfFile::from_split_paths(&path, path_for_symbols.as_deref()) {
                    Ok(elf) => system_elfs.push(elf),
                    Err(e) => {
                        eprintln!(
                            "ERROR: failed to parse ELF '{}' for PD '{}': {}",
                            path.display(),
                            pd.name,
                            e
                        );
                        std::process::exit(1);
                    }
                };
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
            args.config.as_str(),
            &spec_container,
            &system_elfs,
            &mut capdl_initialiser,
        );

        match kernel_boot_type {
            KernelBootType::X86_64 => {
                // setvar region_paddr not supported on this architecture nor can we emulate the
                // kernel boot process to statically check for issues due to unknown memory map, so nothing to do.
                // Write out the capDL initialiser as an ELF boot module and we are done.
            }
            KernelBootType::Static { ref kernel_elf, ref available_memory, kernel_boot_region } => {
                // Now that we have the CapDL initialiser ELF with embedded spec,
                // we can determine exactly how much memory will be available statically when the kernel
                // drops to userspace on ARM and RISC-V. This allow us to sanity check that:
                // 1. We have enough memory to allocate all the objects required in the spec.
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
                let mut available_memory = available_memory.clone();

                // The kernel relies on the initial task region being allocated above the kernel
                // boot/ELF region, so we have the end of the kernel boot region as the lower
                // bound for allocating the reserved region.
                let initial_task_phys_base =
                    available_memory.allocate_from(initial_task_size, kernel_boot_region.end);

                let Some(initial_task_phys_base) = initial_task_phys_base else {
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
                };

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
                    kernel_elf,
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
                // At runtime the initialiser will validate what we simulated against what the kernel gives it. If they deviate
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

            let image_out_path = args.output_path.as_path();

            match kernel_boot_type {
                KernelBootType::X86_64 => match capdl_initialiser.elf.reserialise(image_out_path) {
                    Ok(size) => {
                        // Copy the kernel to the build directory as well so users doesn't have to dig through the SDK.
                        if let Err(copy_err) = fs::copy(
                            kernel_elf_path,
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
                KernelBootType::Static { ref kernel_elf, .. } => {
                    let loader = Loader::new(
                        &kernel_config,
                        Path::new(&loader_elf_path),
                        kernel_elf,
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

            if let Some(capdl_json) = args.capdl_json_path {
                let serialised = serde_json::to_string_pretty(&spec_container.spec).unwrap();
                fs::write(capdl_json, &serialised).unwrap();
            };

            write_report(&spec_container, &kernel_config, &args.report_path);
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
