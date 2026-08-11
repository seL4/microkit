//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::collections::BTreeMap;
use std::rc::Rc;

use super::consts::*;
use super::pd_vm::ProtectionDomain;
use super::util::{check_attributes, checked_lookup, loc_string, value_error};
use super::{SdfNode, SystemDescriptionFile};

use crate::util::str_to_bool;

#[derive(Debug, Clone)]
pub struct ChannelEnd {
    pub pd: Rc<str>,
    pub id: u64,
    pub notify: bool,
    pub pp: bool,
    pub setvar_id: Option<String>,
}

#[derive(Debug)]
pub struct Channel {
    pub end_a: ChannelEnd,
    pub end_b: ChannelEnd,
}

impl ChannelEnd {
    fn from_xml<'a>(
        xml_sdf: &'a SystemDescriptionFile,
        node: &'a dyn SdfNode,
        pds: &BTreeMap<Rc<str>, ProtectionDomain>,
    ) -> Result<ChannelEnd, String> {
        let node_name = node.tag_name();
        if node_name != "end" {
            let pos = node.range().start;
            return Err(format!(
                "Error: invalid XML element '{}': {}",
                node_name,
                loc_string(xml_sdf, pos)
            ));
        }

        check_attributes(xml_sdf, node, &["pd", "id", "pp", "notify", "setvar_id"])?;
        let end_pd = checked_lookup(xml_sdf, node, "pd")?;
        let end_id = checked_lookup(xml_sdf, node, "id")?.parse::<i64>().unwrap();

        if end_id > PD_MAX_ID as i64 {
            return Err(value_error(
                xml_sdf,
                node,
                format!("id must be < {}", PD_MAX_ID + 1),
            ));
        }

        if end_id < 0 {
            return Err(value_error(xml_sdf, node, "id must be >= 0".to_string()));
        }

        let notify = node
            .attribute("notify")
            .map(str_to_bool)
            .unwrap_or(Some(true))
            .ok_or_else(|| {
                value_error(
                    xml_sdf,
                    node,
                    "notify must be 'true' or 'false'".to_string(),
                )
            })?;

        let pp = node
            .attribute("pp")
            .map(str_to_bool)
            .unwrap_or(Some(false))
            .ok_or_else(|| {
                value_error(xml_sdf, node, "pp must be 'true' or 'false'".to_string())
            })?;

        if let Some(pd) = pds.get(end_pd) {
            let setvar_id = node.attribute("setvar_id").map(ToOwned::to_owned);
            Ok(ChannelEnd {
                pd: pd.name.clone(),
                id: end_id.try_into().unwrap(),
                notify,
                pp,
                setvar_id,
            })
        } else {
            Err(value_error(
                xml_sdf,
                node,
                format!("invalid PD name '{end_pd}'"),
            ))
        }
    }
}

impl Channel {
    /// It should be noted that this function assumes that `pds` is populated
    /// with all the Protection Domains that could potentially be connected with
    /// the channel.
    pub(super) fn from_xml<'a>(
        xml_sdf: &'a SystemDescriptionFile,
        node: &'a dyn SdfNode,
        pds: &BTreeMap<Rc<str>, ProtectionDomain>,
    ) -> Result<Channel, String> {
        check_attributes(xml_sdf, node, &[])?;

        let [ref end_a, ref end_b] = node
            .children()
            .map(|node| ChannelEnd::from_xml(xml_sdf, &*node, pds))
            .collect::<Result<Vec<_>, _>>()?[..]
        else {
            return Err(value_error(
                xml_sdf,
                node,
                "exactly two end elements must be specified".to_string(),
            ));
        };

        if end_a.pp && end_b.pp {
            return Err(value_error(
                xml_sdf,
                node,
                "cannot ppc bidirectionally".to_string(),
            ));
        }

        Ok(Channel {
            end_a: end_a.clone(),
            end_b: end_b.clone(),
        })
    }
}
