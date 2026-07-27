//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use super::consts::*;
use super::util::{check_attributes, checked_lookup, loc_string, sdf_parse_number, value_error};
use super::{SdfLocation, SdfNode, XmlSystemDescription};

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum CapMapType {
    Tcb,
    Sc,
    VSpace,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CapMap {
    pub cap_type: CapMapType,
    // FIXME: This is quite a hack. Basically, we need to be able to reference
    // arbitrary PDs, but to gather the index, we need to know all the PDs.
    // However, at the time of parsing the cap maps, we are in the process
    // of parsing all the PDs. In lieu of something better (in my - @midnightveil's
    // opinion, making everything think in terms of PD names, and something
    // that was necessary to do for the multikernel changes); the pd idx will
    // be filled out later during SDF parse process.
    pub pd_name: String,
    pub pd: Option<usize>,
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
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<CapMap, String> {
        // At the moment the four cap maps we support all have the 'pd' element,
        // so we can include it here. When that stops being the case we will
        // have to rework this a bit.
        check_attributes(xml_sdf, node, &["slot", "pd"])?;

        let pd_name = checked_lookup(xml_sdf, node, "pd")?.to_string();

        let slot = sdf_parse_number(checked_lookup(xml_sdf, node, "slot")?, node)?;

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
            cap_type,
            pd_name,
            // FIXME: Hack, filled out later.
            pd: None,
            slot,
            text_pos: node.range().start,
        })
    }
}

impl CSpace {
    pub(super) fn from_xml(
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<Self, String> {
        check_attributes(xml_sdf, node, &[])?;

        let mut cap_maps = vec![];

        for child in node.children() {
            cap_maps.push(match child.tag_name() {
                "cap_tcb" => CapMap::from_xml(CapMapType::Tcb, xml_sdf, &*child)?,
                "cap_sc" => CapMap::from_xml(CapMapType::Sc, xml_sdf, &*child)?,
                "cap_vspace" => CapMap::from_xml(CapMapType::VSpace, xml_sdf, &*child)?,
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
