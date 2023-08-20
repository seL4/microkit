#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from pathlib import Path
import unittest

from sel4coreplat.sysxml import xml2system, UserError, PlatformDescription


plat_desc = PlatformDescription(
    page_sizes = [0x1_000, 0x200_000],
    # The number of CPUs is dependent on the configuration of the platform that is being built.
    # For the tests we just decide a value for this.
    num_cpus = 4,
    kernel_is_hypervisor = True,
)

def _file(filename: str) -> Path:
    return Path(__file__).parent / filename


class ExtendedTestCase(unittest.TestCase):
    def assertStartsWith(self, v, check):
        self.assertTrue(v.startswith(check), f"'{v}' does not start with '{check}'")

    def _check_error(self, filename, message):
        with self.assertRaises(UserError) as e:
            xml2system(_file(filename), plat_desc)
        self.assertStartsWith(str(e.exception), message)

    def _check_missing(self, filename, attr, element):
        expected_message = f"Error: Missing required attribute '{attr}' on element '{element}'"
        self._check_error(filename, expected_message)


class MemoryRegionParseTests(ExtendedTestCase):
    def test_malformed_size(self):
        self._check_error("mr_malformed_size.xml", "Error: invalid literal for int() with base 0: '0x200_000sd' on element 'memory_region'")

    def test_unsupported_page_size(self):
        self._check_error("mr_unsupported_page_size.xml", "Error: page size 0x200001 not supported on element 'memory_region'")

    def test_size_not_multiple_of_page_size(self):
        self._check_error("mr_size_not_multiple_of_page_size.xml", "Error: size is not a multiple of the page size on element 'memory_region'")

    def test_addr_not_aligned_to_page_size(self):
        self._check_error("mr_addr_not_aligned_to_page_size.xml", "Error: phys_addr is not aligned to the page size on element 'memory_region'")

    def test_missing_size(self):
        self._check_missing("mr_missing_size.xml", "size", "memory_region")

    def test_missing_name(self):
        self._check_missing("mr_missing_name.xml", "name", "memory_region")

    def test_invalid_attrs(self):
        self._check_error("mr_invalid_attrs.xml", "Error: invalid attribute 'page_count' on element 'memory_region': ")


class ProtectionDomainParseTests(ExtendedTestCase):
    def test_missing_name(self):
        self._check_missing("pd_missing_name.xml", "name", "protection_domain")

    def test_missing_path(self):
        self._check_missing("pd_missing_path.xml", "path", "program_image")

    def test_missing_mr(self):
        self._check_missing("pd_missing_mr.xml", "mr", "map")

    def test_missing_vaddr(self):
        self._check_missing("pd_missing_vaddr.xml", "vaddr", "map")

    def test_missing_irq(self):
        self._check_missing("pd_missing_irq.xml", "irq", "irq")

    def test_missing_id(self):
        self._check_missing("pd_missing_id.xml", "id", "irq")

    def test_missing_symbol(self):
        self._check_missing("pd_missing_symbol.xml", "symbol", "setvar")

    def test_missing_region_paddr(self):
        self._check_missing("pd_missing_region_paddr.xml", "region_paddr", "setvar")

    def test_duplicate_program_image(self):
        self._check_error("pd_duplicate_program_image.xml", "Error: program_image must only be specified once on element 'program_image': ")

    def test_invalid_attrs(self):
        self._check_error("pd_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'protection_domain': ")

    def test_program_image_invalid_attrs(self):
        self._check_error("pd_program_image_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'program_image': ")

    def test_budget_gt_period(self):
        self._check_error("pd_budget_gt_period.xml", "Error: budget (1000) must be less than, or equal to, period (100) on element 'protection_domain':")

    def test_cpu_greater_than_max(self):
        self._check_error("pd_cpu_greater_than_max.xml", f"Error: CPU affinity must be between 0 and {plat_desc.num_cpus - 1} on element 'protection_domain':")

    def test_cpu_less_than_0(self):
        self._check_error("pd_cpu_less_than_0.xml", f"Error: CPU affinity must be between 0 and {plat_desc.num_cpus - 1} on element 'protection_domain':")

    def test_write_only_mr(self):
        self._check_error("pd_write_only_mr.xml", f"Error: perms must not be 'w', write-only mappings are not allowed on element 'map':")


class VirtualMachineParseTests(ExtendedTestCase):
    def test_duplicate_name(self):
        self._check_error("vm_duplicate_name.xml", f"Duplicate virtual machine name 'test-vm'.")


class ChannelParseTests(ExtendedTestCase):
    def test_missing_pd(self):
        self._check_missing("ch_missing_pd.xml", "pd", "end")

    def test_missing_id(self):
        self._check_missing("ch_missing_id.xml", "id", "end")

    def test_id_greater_than_63(self):
        self._check_error("ch_id_greater_than_63.xml", "Error: id must be < 64 on element 'end'")

    def test_id_less_than_0(self):
        self._check_error("ch_id_less_than_0.xml", "Error: id must be >= 0 on element 'end'")

    def test_invalid_attrs(self):
        self._check_error("ch_invalid_attrs.xml", "Error: invalid attribute 'foo' on element 'channel': ")


class SystemParseTests(ExtendedTestCase):
    def test_duplicate_pd_names(self):
        self._check_error("sys_duplicate_pd_name.xml", "Duplicate protection domain name 'test'.")

    def test_duplicate_mr_names(self):
        self._check_error("sys_duplicate_mr_name.xml", "Duplicate memory region name 'test'.")

    def test_channel_invalid_pd(self):
        self._check_error("sys_channel_invalid_pd.xml", "Protection domain with name 'invalidpd' on element 'channel' does not exist: ")

    def test_duplicate_irq_number(self):
        self._check_error("sys_duplicate_irq_number.xml", "duplicate irq: 112 in protection domain: 'test2' @ ")

    def test_duplicate_irq_id(self):
        self._check_error("sys_duplicate_irq_id.xml", "duplicate channel id: 3 in protection domain: 'test1' @")

    def test_channel_duplicate_a_id(self):
        self._check_error("sys_channel_duplicate_a_id.xml", "duplicate channel id: 5 in protection domain: 'test1' @")

    def test_channel_duplicate_b_id(self):
        self._check_error("sys_channel_duplicate_b_id.xml", "duplicate channel id: 5 in protection domain: 'test2' @")

    def test_no_protection_domains(self):
        self._check_error("sys_no_protection_domains.xml", "At least one protection domain must be defined")

    def test_text_elements(self):
        self._check_error("sys_text_elements.xml", "Error: unexpected text found in element 'system' @")

    def test_map_invalid_mr(self):
        self._check_error("sys_map_invalid_mr.xml", "Invalid memory region name 'foos' on 'map' @ ")

    def test_map_not_aligned(self):
        self._check_error("sys_map_not_aligned.xml", "Invalid vaddr alignment on 'map' @ ")

    def test_too_many_pds(self):
        self._check_error("sys_too_many_pds.xml", "Too many protection domains (64) defined. Maximum is 63.")