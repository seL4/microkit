//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::num::NonZeroU64;
use std::rc::Rc;

use super::consts::*;
use super::util::{
    check_attributes, checked_lookup, loc_string, sdf_parse_required_attribute, value_error,
};
use super::{SdfLocation, SdfNode, SystemDescriptionFile};

use crate::{sel4::Arch, Config};

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum CapMapType {
    Tcb,
    Sc,
    VSpace,
    ArmSmc,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CapMapRefDataPdKind {
    Tcb,
    Sc,
    VSpace,
}

impl From<CapMapType> for CapMapRefDataPdKind {
    fn from(value: CapMapType) -> Self {
        match value {
            CapMapType::Tcb => CapMapRefDataPdKind::Tcb,
            CapMapType::Sc => CapMapRefDataPdKind::Sc,
            CapMapType::VSpace => CapMapRefDataPdKind::VSpace,
            CapMapType::ArmSmc => unreachable!(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CapMapRefData {
    Pd {
        pd: Rc<str>,
        kind: CapMapRefDataPdKind,
    },
    ArmSmcFunction {
        function_id: Option<NonZeroU64>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct CapMap {
    pub ref_data: CapMapRefData,
    // The destination "slot" in the CSpace: note that this is "opaque" and
    // can be shifted depending on the location in the CSpace to work as the CPtr,
    // but here it is given as the index into the CNode.
    pub slot: u64,
    /// Location in the parsed SDF file
    pub text_pos: SdfLocation,
}

#[derive(Debug)]
pub struct CSpace {
    pub cap_maps: Vec<CapMap>,
}

impl CapMap {
    fn from_xml(
        cap_type: CapMapType,
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
    ) -> Result<CapMap, String> {
        let ref_data = match cap_type {
            CapMapType::Tcb | CapMapType::Sc | CapMapType::VSpace => {
                check_attributes(xml_sdf, node, &["slot", "pd"])?;

                let pd = Rc::from(checked_lookup(xml_sdf, node, "pd")?);

                CapMapRefData::Pd {
                    pd,
                    kind: cap_type.into(),
                }
            }
            CapMapType::ArmSmc => {
                check_attributes(xml_sdf, node, &["slot", "function_id"])?;

                if config.arch != Arch::Aarch64 {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "cap_smc is only supported on AArch64".to_string(),
                    ));
                }

                let function_id: u64 = sdf_parse_required_attribute(xml_sdf, node, "function_id")?;
                let function_id = NonZeroU64::new(function_id);

                CapMapRefData::ArmSmcFunction { function_id }
            }
        };

        let slot: u64 = sdf_parse_required_attribute(xml_sdf, node, "slot")?;

        if slot == 0 {
            return Err(value_error(
                xml_sdf,
                node,
                ("The destination slot 0 has been reserved for Microkit CNode").to_string(),
            ));
        }

        // TODO: Rework this so that we don't have a fixed upper limit.
        if slot >= CAP_MAP_MAX_SLOT {
            return Err(value_error(
                xml_sdf,
                node,
                format!("There are only {CAP_MAP_MAX_SLOT} destination cspace slots available."),
            ));
        }

        Ok(CapMap {
            ref_data,
            slot,
            text_pos: node.range().start,
        })
    }

    pub(crate) fn format_for_slot_collision(&self, xml_sdf: &SystemDescriptionFile) -> String {
        let loc = loc_string(xml_sdf, self.text_pos);

        match &self.ref_data {
            CapMapRefData::Pd { pd, kind, .. } => {
                format!("pd '{pd}'s {kind:?} at '{loc}'")
            }
            CapMapRefData::ArmSmcFunction { function_id } => {
                format!(
                    "smc cap for function '{} at '{}'",
                    function_id.map(NonZeroU64::get).unwrap_or(0),
                    loc
                )
            }
        }
    }
}

impl CSpace {
    pub(super) fn from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
    ) -> Result<Self, String> {
        check_attributes(xml_sdf, node, &[])?;

        let mut cap_maps = vec![];

        for child in node.children() {
            cap_maps.push(match child.tag_name() {
                "cap_tcb" => CapMap::from_xml(CapMapType::Tcb, config, xml_sdf, &*child)?,
                "cap_sc" => CapMap::from_xml(CapMapType::Sc, config, xml_sdf, &*child)?,
                "cap_vspace" => CapMap::from_xml(CapMapType::VSpace, config, xml_sdf, &*child)?,
                "cap_smc" => CapMap::from_xml(CapMapType::ArmSmc, config, xml_sdf, &*child)?,
                child_name => {
                    let location = loc_string(xml_sdf, child.range().start);
                    if let Some(type_name) = child_name.strip_prefix("cap_") {
                        return Err(format!("Cap type: '{type_name}' is not supported at '{location}'"));
                    } else {
                        return Err(format!("Element '{child_name}' is not supported in a <cspace> element at '{location}'"));
                    }
                }
            })
        }

        Ok(CSpace { cap_maps })
    }
}
