//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use super::memory_region::SysIOMap;
use super::pci::{PciDevice, PciDeviceParseError};
use super::util::{
    check_attributes, checked_lookup, loc_string, sdf_parse_required_attribute, value_error,
};
use super::{SdfNode, SystemDescriptionFile};

use crate::{sel4::Arch, Config};

// This is implemented in such a way that each device will have its own address space.
// If devices need to share physical memory, this can be done by mapping the same memory_region
// into each address space.
pub struct IOAddressSpace {
    pub iomaps: Vec<SysIOMap>,
}

// This can be extended in future to support devices on an SMMU enabled Arm device
// or IOMMU enabled RISC-V device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuDeviceIdentifier {
    X86Pci(PciDevice),
}

#[derive(Clone, PartialEq, Eq)]
pub enum IommuDeviceIdentifierParseError {
    UnsupportedArch(Arch),
    Pci(PciDeviceParseError),
}

impl IommuDeviceIdentifier {
    fn from_str_for_arch(
        config: &Config,
        s: &str,
    ) -> Result<Self, IommuDeviceIdentifierParseError> {
        match config.arch {
            Arch::X86_64 => PciDevice::from_str(s)
                .map(IommuDeviceIdentifier::X86Pci)
                .map_err(IommuDeviceIdentifierParseError::Pci),
            Arch::Aarch64 | Arch::Riscv64 => Err(IommuDeviceIdentifierParseError::UnsupportedArch(
                config.arch,
            )),
        }
    }
}

impl fmt::Display for IommuDeviceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IommuDeviceIdentifier::X86Pci(pci_device) => write!(f, "PCI device {pci_device}"),
        }
    }
}

impl fmt::Display for IommuDeviceIdentifierParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IommuDeviceIdentifierParseError::UnsupportedArch(arch) => {
                write!(f, "IOMMU device identifiers are not supported on {arch}")
            }
            IommuDeviceIdentifierParseError::Pci(err) => write!(f, "{err}"),
        }
    }
}

impl IOAddressSpace {
    pub(super) fn from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
        names: &mut BTreeSet<String>,
        domain_ids: &mut BTreeSet<u64>,
        iommu_device_identifiers: &mut Vec<IommuDeviceIdentifier>,
    ) -> Result<IOAddressSpace, String> {
        if !config.iommu {
            let pos = node.range().start;
            return Err(format!(
                "Error: io address space requires seL4 to be built with IOMMU support: {}",
                loc_string(xml_sdf, pos)
            ));
        }

        check_attributes(xml_sdf, node, &["name", "peripheral_id", "domain_id"])?;
        let name = checked_lookup(xml_sdf, node, "name")?;
        if !names.insert(name.to_string()) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("duplicate name '{name}'"),
            ));
        }

        // Currently we enforce unique domain ids. To support shared domain ids, we have to ensure
        // each device has a duplicated copy of the whole page table structure due to how Intel
        // implements IOMMU caching see section 6.2.1 in the Virtualization Technology (Intel® VT) for Directed I/O
        // (Intel® VT-d) manual:
        // http://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/vt-directed-io-spec.pdf
        let domain_id = match config.arch {
            Arch::X86_64 => {
                let domain_id = sdf_parse_required_attribute(xml_sdf, node, "domain_id")?;

                if !domain_ids.insert(domain_id) {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "reusing a domain id is forbidden".into(),
                    ));
                }
                Some(domain_id)
            }
            _ => None,
        };

        // In the SDF we use peripheral_id as an architecture agnostic way to describe
        // how a device is identified in a system. For example on x86 the IOMMU identifies
        // devices by the PCI tuple (bus,dev,fn)
        let identifier_str = checked_lookup(xml_sdf, node, "peripheral_id")?;
        let identifier =
            IommuDeviceIdentifier::from_str_for_arch(config, identifier_str).map_err(|err| {
                value_error(
                    xml_sdf,
                    node,
                    format!("failed to parse device peripheral_id '{identifier_str}': {err}"),
                )
            })?;
        if iommu_device_identifiers.contains(&identifier) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("duplicate device peripheral_id '{identifier}'"),
            ));
        }
        iommu_device_identifiers.push(identifier);

        let mut iomaps = Vec::new();

        for child in node.children() {
            match child.tag_name() {
                "iomap" => {
                    let iomap =
                        SysIOMap::from_xml(config, xml_sdf, &*child, name, identifier, domain_id)?;
                    iomaps.push(iomap);
                }
                _ => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element '{}': {}",
                        child.tag_name(),
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        Ok(IOAddressSpace { iomaps })
    }
}
