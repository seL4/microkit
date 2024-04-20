use microkit_tool::{sysxml, sel4};

const DEFAULT_KERNEL_CONFIG: sel4::Config = sel4::Config {
    arch: sel4::Arch::Aarch64,
    word_size: 64,
    minimum_page_size: 4096,
    paddr_user_device_top: 1 << 40,
    kernel_frame_size: 1 << 12,
    init_cnode_bits: 12,
    cap_address_bits: 64,
    fan_out_limit: 256,
};

const DEFAULT_PLAT_DESC: sysxml::PlatformDescription = sysxml::PlatformDescription::new(&DEFAULT_KERNEL_CONFIG);

fn check_error(test_name: &str, err: &str) {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/sdf/");
    path.push(test_name);
    let xml = std::fs::read_to_string(path).unwrap();
    let parse_err = sysxml::parse(test_name, &xml, &DEFAULT_PLAT_DESC).unwrap_err();
    assert!(parse_err.starts_with(err));
}

fn check_missing(test_name: &str, attr: &str, element: &str) {
    let expected_error = format!("Error: Missing required attribute '{}' on element '{}'", attr, element);
    check_error(test_name, expected_error.as_str());
}

#[cfg(test)]
mod memory_region {
    use super::*;

    #[test]
    fn test_malformed_size() {
        check_error("mr_malformed_size.xml", "Error: failed to parse integer '0x200_000sd' on element 'memory_region': invalid digit found in string")
    }

    #[test]
    fn test_unsupported_page_size() {
        check_error("mr_unsupported_page_size.xml", "Error: page size 0x200001 not supported on element 'memory_region'")
    }

    #[test]
    fn test_size_not_multiple_of_page_size() {
        check_error("mr_size_not_multiple_of_page_size.xml", "Error: size is not a multiple of the page size on element 'memory_region'")
    }

    #[test]
    fn test_addr_not_aligned_to_page_size() {
        check_error("mr_addr_not_aligned_to_page_size.xml", "Error: phys_addr is not aligned to the page size on element 'memory_region'")
    }

    #[test]
    fn test_missing_size() {
        check_missing("mr_missing_size.xml", "size", "memory_region")
    }

    #[test]
    fn test_missing_name() {
        check_missing("mr_missing_name.xml", "name", "memory_region")
    }

    #[test]
    fn test_invalid_attrs() {
        check_error("mr_invalid_attrs.xml", "Error: invalid attribute 'page_count' on element 'memory_region': ")
    }
}

#[cfg(test)]
mod protection_domain {
    use super::*;

    #[test]
    fn test_missing_name() {
        check_missing("pd_missing_name.xml", "name", "protection_domain")
    }

    #[test]
    fn test_missing_path() {
        check_missing("pd_missing_path.xml", "path", "program_image")
    }

    #[test]
    fn test_missing_mr() {
        check_missing("pd_missing_mr.xml", "mr", "map")
    }

    #[test]
    fn test_missing_vaddr() {
        check_missing("pd_missing_vaddr.xml", "vaddr", "map")
    }

    #[test]
    fn test_missing_irq() {
        check_missing("pd_missing_irq.xml", "irq", "irq")
    }

    #[test]
    fn test_missing_id() {
        check_missing("pd_missing_id.xml", "id", "irq")
    }

    #[test]
    fn test_missing_symbol() {
        check_missing("pd_missing_symbol.xml", "symbol", "setvar")
    }

    #[test]
    fn test_missing_region_paddr() {
        check_missing("pd_missing_region_paddr.xml", "region_paddr", "setvar")
    }

    #[test]
    fn test_duplicate_program_image() {
        check_error("pd_duplicate_program_image.xml", "Error: program_image must only be specified once on element 'program_image': ")
    }

    #[test]
    fn test_invalid_attrs() {
        check_error("pd_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'protection_domain': ")
    }

    #[test]
    fn test_program_image_invalid_attrs() {
        check_error("pd_program_image_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'program_image': ")
    }

    #[test]
    fn test_budget_gt_period() {
        check_error("pd_budget_gt_period.xml", "Error: budget (1000) must be less than, or equal to, period (100) on element 'protection_domain':")
    }

    #[test]
    fn test_irq_greater_than_62() {
        check_error("irq_id_greater_than_62.xml", "Error: id must be < 63 on element 'irq'")
    }

    #[test]
    fn test_irq_less_than_0() {
        check_error("irq_id_less_than_0.xml", "Error: id must be >= 0 on element 'irq'")
    }

    #[test]
    fn test_write_only_mr() {
        check_error("pd_write_only_mr.xml", "Error: perms must not be 'w', write-only mappings are not allowed on element 'map':")
    }

    #[test]
    fn test_irq_invalid_trigger() {
        check_error("irq_invalid_trigger.xml", "Error: trigger must be either 'level' or 'edge' on element 'irq'")
    }
}

#[cfg(test)]
mod channel {
    use super::*;

    #[test]
    fn test_missing_pd() {
        check_missing("ch_missing_pd.xml", "pd", "end")
    }

    #[test]
    fn test_missing_id() {
        check_missing("ch_missing_id.xml", "id", "end")
    }

    #[test]
    fn test_id_greater_than_max() {
        check_error("ch_id_greater_than_62.xml", "Error: id must be < 63 on element 'end'")
    }

    #[test]
    fn test_id_less_than_0() {
        check_error("ch_id_less_than_0.xml", "Error: id must be >= 0 on element 'end'")
    }

    #[test]
    fn test_invalid_attrs() {
        check_error("ch_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'channel': ")
    }

    #[test]
    fn test_channel_invalid_pd() {
        check_error("ch_invalid_pd.xml", "Error: invalid PD name 'invalidpd' on element 'end': ")
    }
}

#[cfg(test)]
mod system {
    use super::*;

    #[test]
    fn test_duplicate_pd_names() {
        check_error("sys_duplicate_pd_name.xml", "Error: duplicate protection domain name 'test'.")
    }

    #[test]
    fn test_duplicate_mr_names() {
        check_error("sys_duplicate_mr_name.xml", "Error: duplicate memory region name 'test'.")
    }

    #[test]
    fn test_duplicate_irq_number() {
        check_error("sys_duplicate_irq_number.xml", "Error: duplicate irq: 112 in protection domain: 'test2' @ ")
    }

    #[test]
    fn test_duplicate_irq_id() {
        check_error("sys_duplicate_irq_id.xml", "Error: duplicate channel id: 3 in protection domain: 'test1' @")
    }

    #[test]
    fn test_channel_duplicate_a_id() {
        check_error("sys_channel_duplicate_a_id.xml", "Error: duplicate channel id: 5 in protection domain: 'test1' @")
    }

    #[test]
    fn test_channel_duplicate_b_id() {
        check_error("sys_channel_duplicate_b_id.xml", "Error: duplicate channel id: 5 in protection domain: 'test2' @")
    }

    #[test]
    fn test_no_protection_domains() {
        check_error("sys_no_protection_domains.xml", "Error: at least one protection domain must be defined")
    }

    #[test]
    fn test_text_elements() {
        check_error("sys_text_elements.xml", "Error: unexpected text found in element 'system' @")
    }

    #[test]
    fn test_map_invalid_mr() {
        check_error("sys_map_invalid_mr.xml", "Error: invalid memory region name 'foos' on 'map' @ ")
    }

    #[test]
    fn test_map_not_aligned() {
        check_error("sys_map_not_aligned.xml", "Error: invalid vaddr alignment on 'map' @ ")
    }

    #[test]
    fn test_too_many_pds() {
        check_error("sys_too_many_pds.xml", "Error: too many protection domains (64) defined. Maximum is 63.")
    }
}
