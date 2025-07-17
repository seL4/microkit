//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use microkit_tool::{sdf, sel4};
use serde_json::json;

const DEFAULT_KERNEL_CONFIG: sel4::Config = sel4::Config {
    arch: sel4::Arch::Aarch64,
    word_size: 64,
    minimum_page_size: 4096,
    paddr_user_device_top: 1 << 40,
    kernel_frame_size: 1 << 12,
    init_cnode_bits: 12,
    cap_address_bits: 64,
    fan_out_limit: 256,
    hypervisor: true,
    microkit_config: sel4::MicrokitConfig::Debug,
    fpu: true,
    arm_pa_size_bits: Some(40),
    arm_smc: None,
    riscv_pt_levels: None,
    // Not necessary for SDF parsing
    invocations_labels: json!(null),
    device_regions: vec![],
    normal_regions: vec![],
};

fn check_error(test_name: &str, expected_err: &str) {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/sdf/");
    path.push(test_name);
    let sdf = std::fs::read_to_string(path).unwrap();
    let parse_err = sdf::parse(test_name, &sdf, &DEFAULT_KERNEL_CONFIG).unwrap_err();

    if !parse_err.starts_with(expected_err) {
        eprintln!(
            "Expected error:\n{}\nGot error:\n{}\n",
            expected_err, parse_err
        );
    }

    assert!(parse_err.starts_with(expected_err));
}

fn check_missing(test_name: &str, attr: &str, element: &str) {
    let expected_error = format!(
        "Error: Missing required attribute '{}' on element '{}'",
        attr, element
    );
    check_error(test_name, expected_error.as_str());
}

#[cfg(test)]
mod memory_region {
    use super::*;

    #[test]
    fn test_malformed_size() {
        check_error("mr_malformed_size.system", "Error: failed to parse integer '0x200_000sd' on element 'memory_region': invalid digit found in string")
    }

    #[test]
    fn test_unsupported_page_size() {
        check_error(
            "mr_unsupported_page_size.system",
            "Error: page size 0x200001 not supported on element 'memory_region'",
        )
    }

    #[test]
    fn test_size_not_multiple_of_page_size() {
        check_error(
            "mr_size_not_multiple_of_page_size.system",
            "Error: size is not a multiple of the page size on element 'memory_region'",
        )
    }

    #[test]
    fn test_addr_not_aligned_to_page_size() {
        check_error(
            "mr_addr_not_aligned_to_page_size.system",
            "Error: phys_addr is not aligned to the page size on element 'memory_region'",
        )
    }

    #[test]
    fn test_missing_size() {
        check_missing("mr_missing_size.system", "size", "memory_region")
    }

    #[test]
    fn test_missing_name() {
        check_missing("mr_missing_name.system", "name", "memory_region")
    }

    #[test]
    fn test_invalid_attrs() {
        check_error(
            "mr_invalid_attrs.system",
            "Error: invalid attribute 'page_count' on element 'memory_region': ",
        )
    }

    #[test]
    fn test_overlapping_phys_addr() {
        check_error(
            "mr_overlapping_phys_addr.system",
            "Error: memory region 'mr2' physical address range [0x9001000..0x9002000) overlaps with another memory region 'mr1' [0x9000000..0x9002000) @ ",
        )
    }
}

#[cfg(test)]
mod protection_domain {
    use super::*;

    #[test]
    fn test_missing_name() {
        check_missing("pd_missing_name.system", "name", "protection_domain")
    }

    #[test]
    fn test_missing_program_image() {
        check_error(
            "pd_missing_program_image.system",
            "Error: missing 'program_image' element on protection_domain: ",
        )
    }

    #[test]
    fn test_missing_path() {
        check_missing("pd_missing_path.system", "path", "program_image")
    }

    #[test]
    fn test_missing_mr() {
        check_missing("pd_missing_mr.system", "mr", "map")
    }

    #[test]
    fn test_missing_vaddr() {
        check_missing("pd_missing_vaddr.system", "vaddr", "map")
    }

    #[test]
    fn test_missing_irq() {
        check_missing("pd_missing_irq.system", "irq", "irq")
    }

    #[test]
    fn test_missing_id() {
        check_missing("pd_missing_id.system", "id", "irq")
    }

    #[test]
    fn test_missing_symbol() {
        check_missing("pd_missing_symbol.system", "symbol", "setvar")
    }

    #[test]
    fn test_missing_region_paddr() {
        check_missing("pd_missing_region_paddr.system", "region_paddr", "setvar")
    }

    #[test]
    fn test_duplicate_setvar() {
        check_error(
            "pd_duplicate_setvar.system",
            "Error: setvar on symbol 'test' already exists on element 'setvar': ",
        )
    }

    #[test]
    fn test_duplicate_program_image() {
        check_error(
            "pd_duplicate_program_image.system",
            "Error: program_image must only be specified once on element 'protection_domain': ",
        )
    }

    #[test]
    fn test_invalid_attrs() {
        check_error(
            "pd_invalid_attrs.system",
            "Error: invalid attribute 'foo' on element 'protection_domain': ",
        )
    }

    #[test]
    fn test_program_image_invalid_attrs() {
        check_error(
            "pd_program_image_invalid_attrs.system",
            "Error: invalid attribute 'foo' on element 'program_image': ",
        )
    }

    #[test]
    fn test_budget_gt_period() {
        check_error("pd_budget_gt_period.system", "Error: budget (1000) must be less than, or equal to, period (100) on element 'protection_domain':")
    }

    #[test]
    fn test_irq_greater_than_max() {
        check_error(
            "irq_id_greater_than_max.system",
            "Error: id must be < 62 on element 'irq'",
        )
    }

    #[test]
    fn test_irq_less_than_0() {
        check_error(
            "irq_id_less_than_0.system",
            "Error: id must be >= 0 on element 'irq'",
        )
    }

    #[test]
    fn test_write_only_mr() {
        check_error(
            "pd_write_only_mr.system",
            "Error: perms must not be 'w', write-only mappings are not allowed on element 'map':",
        )
    }

    #[test]
    fn test_irq_invalid_trigger() {
        check_error(
            "irq_invalid_trigger.system",
            "Error: trigger must be either 'level' or 'edge' on element 'irq'",
        )
    }

    #[test]
    fn test_parent_has_id() {
        check_error(
            "pd_parent_has_id.system",
            "Error: invalid attribute 'id' on element 'protection_domain': ",
        )
    }

    #[test]
    fn test_child_missing_id() {
        check_missing("pd_child_missing_id.system", "id", "protection_domain")
    }

    #[test]
    fn test_duplicate_child_id() {
        check_error(
            "pd_duplicate_child_id.system",
            "Error: duplicate id: 0 in protection domain: 'parent' @",
        )
    }

    #[test]
    fn test_duplicate_child_id_vcpu() {
        check_error(
            "pd_duplicate_child_id_vcpu.system",
            "Error: duplicate id: 0 clashes with virtual machine vcpu id in protection domain: 'parent' @",
        )
    }

    #[test]
    fn test_small_stack_size() {
        check_error(
            "pd_small_stack_size.system",
            "Error: stack size must be between",
        )
    }

    #[test]
    fn test_unaligned_stack_size() {
        check_error(
            "pd_unaligned_stack_size.system",
            "Error: stack size must be aligned to the smallest page size",
        )
    }

    #[test]
    fn test_overlapping_maps() {
        check_error(
            "pd_overlapping_maps.system",
            "Error: map for 'mr2' has virtual address range [0x1000000..0x1001000) which overlaps with map for 'mr1' [0x1000000..0x1001000) in protection domain 'hello' @"
        )
    }
}

#[cfg(test)]
mod virtual_machine {
    use super::*;

    #[test]
    fn test_vm_not_child() {
        check_error(
            "vm_not_child.system",
            "Error: virtual machine must be a child of a protection domain",
        )
    }

    #[test]
    fn test_duplicate_name() {
        check_error(
            "vm_duplicate_name.system",
            "Error: duplicate virtual machine name 'guest'",
        )
    }

    #[test]
    fn test_missing_vcpu() {
        check_error(
            "vm_missing_vcpu.system",
            "Error: missing 'vcpu' element on virtual_machine: ",
        )
    }

    #[test]
    fn test_missing_vcpu_id() {
        check_missing("vm_missing_vcpu_id.system", "id", "vcpu")
    }

    #[test]
    fn test_invalid_vcpu_id() {
        check_error(
            "vm_invalid_vcpu_id.system",
            "Error: id must be < 62 on element 'vcpu'",
        )
    }

    #[test]
    fn test_overlapping_maps() {
        check_error(
            "vm_overlapping_maps.system",
            "Error: map for 'mr2' has virtual address range [0x1000000..0x1001000) which overlaps with map for 'mr1' [0x1000000..0x1001000) in virtual machine 'guest' @"
        )
    }

    #[test]
    fn test_missing_mr() {
        check_error(
            "vm_missing_mr.system",
            "Error: invalid memory region name 'mr1' on 'map' @",
        )
    }
}

#[cfg(test)]
mod channel {
    use super::*;

    #[test]
    fn test_missing_id() {
        check_missing("ch_missing_id.system", "id", "end")
    }

    #[test]
    fn test_id_greater_than_max() {
        check_error(
            "ch_id_greater_than_max.system",
            "Error: id must be < 62 on element 'end'",
        )
    }

    #[test]
    fn test_id_less_than_0() {
        check_error(
            "ch_id_less_than_0.system",
            "Error: id must be >= 0 on element 'end'",
        )
    }

    #[test]
    fn test_invalid_attrs() {
        check_error(
            "ch_invalid_attrs.system",
            "Error: invalid attribute 'foo' on element 'channel': ",
        )
    }

    #[test]
    fn test_channel_invalid_pd() {
        check_error(
            "ch_invalid_pd.system",
            "Error: invalid PD name 'invalidpd' on element 'end': ",
        )
    }

    #[test]
    fn test_invalid_element() {
        check_error(
            "ch_invalid_element.system",
            "Error: invalid XML element 'ending': ",
        )
    }

    #[test]
    fn test_not_enough_ends() {
        check_error(
            "ch_not_enough_ends.system",
            "Error: exactly two end elements must be specified on element 'channel': ",
        )
    }

    #[test]
    fn test_too_many_ends() {
        check_error(
            "ch_too_many_ends.system",
            "Error: exactly two end elements must be specified on element 'channel': ",
        )
    }

    #[test]
    fn test_end_invalid_pp() {
        check_error(
            "ch_end_invalid_pp.system",
            "Error: pp must be 'true' or 'false' on element 'end': ",
        )
    }

    #[test]
    fn test_end_invalid_notify() {
        check_error(
            "ch_end_invalid_notify.system",
            "Error: notify must be 'true' or 'false' on element 'end': ",
        )
    }

    #[test]
    fn test_bidirectional_ppc() {
        check_error(
            "ch_bidirectional_ppc.system",
            "Error: cannot ppc bidirectionally on element 'channel': ",
        )
    }

    #[test]
    fn test_ppcall_priority() {
        check_error(
            "ch_ppcall_priority.system",
            "Error: PPCs must be to protection domains of strictly higher priorities; channel with PPC exists from pd test1 (priority: 2) to pd test2 (priority: 1)",
        )
    }
}

#[cfg(test)]
mod system {
    use super::*;

    #[test]
    fn test_duplicate_pd_names() {
        check_error(
            "sys_duplicate_pd_name.system",
            "Error: duplicate protection domain name 'test'.",
        )
    }

    #[test]
    fn test_duplicate_mr_names() {
        check_error(
            "sys_duplicate_mr_name.system",
            "Error: duplicate memory region name 'test'.",
        )
    }

    #[test]
    fn test_duplicate_irq_number() {
        check_error(
            "sys_duplicate_irq_number.system",
            "Error: duplicate irq: 112 in protection domain: 'test2' @ ",
        )
    }

    #[test]
    fn test_duplicate_irq_id() {
        check_error(
            "sys_duplicate_irq_id.system",
            "Error: duplicate channel id: 3 in protection domain: 'test1' @",
        )
    }

    #[test]
    fn test_channel_duplicate_a_id() {
        check_error(
            "sys_channel_duplicate_a_id.system",
            "Error: duplicate channel id: 5 in protection domain: 'test1' @",
        )
    }

    #[test]
    fn test_channel_duplicate_b_id() {
        check_error(
            "sys_channel_duplicate_b_id.system",
            "Error: duplicate channel id: 5 in protection domain: 'test2' @",
        )
    }

    #[test]
    fn test_no_protection_domains() {
        check_error(
            "sys_no_protection_domains.system",
            "Error: at least one protection domain must be defined",
        )
    }

    #[test]
    fn test_text_elements() {
        check_error(
            "sys_text_elements.system",
            "Error: unexpected text found in element 'system' @",
        )
    }

    #[test]
    fn test_map_invalid_mr() {
        check_error(
            "sys_map_invalid_mr.system",
            "Error: invalid memory region name 'foos' on 'map' @ ",
        )
    }

    #[test]
    fn test_map_not_aligned() {
        check_error(
            "sys_map_not_aligned.system",
            "Error: invalid vaddr alignment on 'map' @ ",
        )
    }

    #[test]
    fn test_map_too_high() {
        check_error(
            "sys_map_too_high.system",
            "Error: vaddr (0x1000000000000000) must be less than 0xfffffff000 on element 'map'",
        )
    }

    #[test]
    fn test_too_many_pds() {
        check_error(
            "sys_too_many_pds.system",
            "Error: too many protection domains (64) defined. Maximum is 63.",
        )
    }
}
