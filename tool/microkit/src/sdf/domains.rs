//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::collections::{btree_map, BTreeMap};
use std::num::NonZero;

use sel4_capdl_initializer_types::{DomainSchedDuration, DomainSchedEntry};

use super::util::{check_attributes, checked_lookup, loc_string, sdf_parse_number, value_error};
use super::{SdfNode, SystemDescriptionFile};

use crate::Config;

#[derive(Debug, Default)]
pub struct Domains {
    pub name_to_id_map: BTreeMap<String, u8>,
    pub schedule_set_start: Option<u64>,
    pub schedule_index_shift: Option<u64>,
    pub schedule: Vec<DomainSchedEntry>,
}

impl Domains {
    pub(super) fn from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
    ) -> Result<Self, String> {
        check_attributes(xml_sdf, node, &[])?;

        if config.num_cores != 1 {
            return Err(
                "Error: The domain scheduler is not supported in multicore builds of seL4"
                    .to_string(),
            );
        }

        let mut name_to_id_map = BTreeMap::<String, Option<u8>>::new();
        let mut id_to_name_map = BTreeMap::<u8, String>::new();
        let mut domain_schedule_element = None;

        for child in node.children() {
            match child.tag_name() {
                "domain" => {
                    let (dom_name, dom_id) = Self::domain_from_xml(config, xml_sdf, &*child)?;

                    if let Some(existing_dom) = name_to_id_map.insert(dom_name.clone(), dom_id) {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            format!(
                                "Each <domain>'s name element must be unique \
                                 found existing domain '{dom_name}' with id '{existing_dom:?}'"
                            ),
                        ));
                    }

                    if let Some(dom_id) = dom_id {
                        if let Some(existing_dom) = id_to_name_map.insert(dom_id, dom_name.clone())
                        {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!(
                                    "Each <domain>'s id element must be unique \
                                     found existing domain '{existing_dom}' with id '{dom_id}'"
                                ),
                            ));
                        }
                    }
                }
                "domain_schedule" => {
                    if domain_schedule_element.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            "The <domain_schedule> element can only appear once".to_string(),
                        ));
                    }

                    domain_schedule_element = Some(child);
                }
                _ => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element as child of <domains> '{}': {}",
                        child.tag_name(),
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        let Some(domain_schedule_element) = domain_schedule_element else {
            return Err(value_error(
                xml_sdf,
                node,
                "The <domain_schedule> element must appear once".to_string(),
            ));
        };

        let name_to_id_map = name_to_id_map
            .into_iter()
            .map(|(name, dom)| match dom {
                Some(dom) => Ok((name, dom)),
                None => {
                    // TODO: We could be more efficient here. However, for a
                    // maximum of 256 possible domains, iterating over the
                    // valid possible domain IDs is actually OK.

                    let mut dom = None;
                    for i in 0..=config.num_domains {
                        if let btree_map::Entry::Vacant(e) = id_to_name_map.entry(i) {
                            e.insert(name.clone());
                            dom = Some(i);
                            break;
                        }
                    }

                    let Some(dom) = dom else {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            format!("Number of domains exceeds {}", config.num_domains),
                        ));
                    };

                    Ok((name, dom))
                }
            })
            .collect::<Result<_, _>>()?;

        Self::domain_schedule_from_xml(config, xml_sdf, &*domain_schedule_element, name_to_id_map)
    }

    fn domain_from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
    ) -> Result<(String, Option<u8>), String> {
        check_attributes(xml_sdf, node, &["name", "id"])?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

        let domain_id = node
            .attribute("id")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?
            .map(|n| {
                if n >= config.num_domains.into() {
                    Err(value_error(
                        xml_sdf,
                        node,
                        format!(
                            "domain id {n} should be less than the \
                             configured KernelNumDomains value of {}",
                            config.num_domains
                        ),
                    ))
                } else {
                    Ok(n.try_into()
                        .expect("num_domains is u8 so by if above this is OK"))
                }
            })
            .transpose()?;

        Ok((name, domain_id))
    }

    fn domain_schedule_from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
        name_to_id_map: BTreeMap<String, u8>,
    ) -> Result<Domains, String> {
        check_attributes(xml_sdf, node, &["index_shift", "start_index"])?;

        let schedule_start_index = node
            .attribute("start_index")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?
            // The domain schedule is only started when the start index is Some(...)
            // so even when not specified we default to a start index of zero.
            .unwrap_or(0);

        let schedule_index_shift = node
            .attribute("index_shift")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?;

        let mut schedule = vec![];

        for child in node.children() {
            match child.tag_name() {
                "schedule_entry" => {
                    schedule.push(Self::schedule_entry_from_xml(
                        xml_sdf,
                        &*child,
                        &name_to_id_map,
                    )?);
                }
                "schedule_end_marker" => {
                    check_attributes(xml_sdf, &*child, &[])?;

                    schedule.push(DomainSchedEntry {
                        domain: 0,
                        duration: DomainSchedDuration::EndMarker,
                    });
                }
                name => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element as child of <domain_schedule> '{name}': {}",
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        if schedule.len() >= config.num_domain_schedules.try_into().unwrap() {
            return Err(format!(
                "More than configured KernelNumDomainSchedules {} \
                number of <schedule_entry> elements found",
                config.num_domain_schedules
            ));
        }

        if schedule_start_index >= schedule.len().try_into().unwrap() {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "schedule_start_index '{schedule_start_index}' is \
                     greater than the length of the schedule '{}'",
                    schedule.len()
                ),
            ));
        }

        if let Some(shift) = schedule_index_shift {
            if shift + u64::try_from(schedule.len()).unwrap() >= config.num_domain_schedules {
                return Err(value_error(
                    xml_sdf,
                    node,
                    format!(
                        "schedule_index_shift '{schedule_start_index}' on top of \
                         the schedule length '{}' would exceed than the configured \
                         KernelNumDomainSchedules {}",
                        schedule.len(),
                        config.num_domain_schedules
                    ),
                ));
            }
        }

        Ok(Domains {
            name_to_id_map,
            schedule_set_start: Some(schedule_start_index),
            schedule_index_shift,
            schedule,
        })
    }

    fn schedule_entry_from_xml(
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
        name_to_id_map: &BTreeMap<String, u8>,
    ) -> Result<DomainSchedEntry, String> {
        check_attributes(xml_sdf, node, &["domain", "duration"])?;

        let domain_name = checked_lookup(xml_sdf, node, "domain")?;
        let duration_str = checked_lookup(xml_sdf, node, "duration")?;

        let &domain = name_to_id_map.get(domain_name).ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!("domain '{domain_name}' does not exist,"),
            )
        })?;

        let (duration_raw, duration_unit) = duration_str.split_once(" ").ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!(
                    "The duration '{duration_str}' must contain a value and a unit, e.g. '1000 us'"
                ),
            )
        })?;

        let duration_int = sdf_parse_number(duration_raw, node)?;
        let duration = NonZero::new(duration_int).ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!("The duration '{duration_str}' must be non-zero"),
            )
        })?;

        let duration = match duration_unit {
            "us" => Ok(DomainSchedDuration::Us(duration)),
            "ticks" => Ok(DomainSchedDuration::Ticks(duration)),
            _ => Err(value_error(
                xml_sdf,
                node,
                format!("The duration '{duration_str}' must be in either 'ticks' or 'us'"),
            )),
        }?;

        Ok(DomainSchedEntry { domain, duration })
    }

    pub fn has_domains(&self) -> bool {
        !self.name_to_id_map.is_empty()
    }
}
