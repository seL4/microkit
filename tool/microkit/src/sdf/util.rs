//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use super::{SdfLocation, SdfNode, SysSetVar, XmlSystemDescription};

/// The purpose of this function is to parse an integer that could
/// either be in decimal or hex format, unlike the normal parsing
/// functionality that the Rust standard library provides.
/// This also removes any underscores that may be present in the number
/// Always returns a base 10 integer.
pub fn sdf_parse_number(s: &str, node: &dyn SdfNode) -> Result<u64, String> {
    let mut to_parse = s.to_string();
    to_parse.retain(|c| c != '_');

    let (final_str, base) = match to_parse.strip_prefix("0x") {
        Some(stripped) => (stripped, 16),
        None => (to_parse.as_str(), 10),
    };

    match u64::from_str_radix(final_str, base) {
        Ok(value) => Ok(value),
        Err(err) => Err(format!(
            "Error: failed to parse integer '{}' on element '{}': {}",
            s,
            node.tag_name(),
            err
        )),
    }
}

pub fn loc_string(xml_sdf: &XmlSystemDescription, pos: SdfLocation) -> String {
    format!("{}:{}:{}", xml_sdf.filename.display(), pos.row, pos.col)
}

pub fn checked_add_setvar(
    setvars: &mut Vec<SysSetVar>,
    setvar: SysSetVar,
    xml_sdf: &XmlSystemDescription<'_>,
    node: &dyn SdfNode<'_>,
) -> Result<(), String> {
    // Check that the symbol does not already exist
    for other_setvar in setvars.iter() {
        if setvar.symbol == other_setvar.symbol {
            return Err(value_error(
                xml_sdf,
                node,
                format!("setvar on symbol '{}' already exists", setvar.symbol),
            ));
        }
    }

    setvars.push(setvar);

    Ok(())
}

pub fn check_no_text(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node) -> Result<(), String> {
    let name = node.tag_name().name();
    let pos = xml_sdf.doc.text_pos_at(node.range().start);
    let pos = SdfLocation {
        row: pos.row,
        col: pos.col,
    };

    if let Some(text) = node.text() {
        // If the text is just whitespace then it is okay
        if !text.trim().is_empty() {
            return Err(format!(
                "Error: unexpected text found in element '{}' @ {}",
                name,
                loc_string(xml_sdf, pos)
            ));
        }
    }

    if node.tail().is_some() {
        return Err(format!(
            "Error: unexpected text found after element '{}' @ {}",
            name,
            loc_string(xml_sdf, pos)
        ));
    }

    for child in node.children() {
        if !child.is_comment() && !child.is_element() {
            check_no_text(xml_sdf, &child)?;
        }
    }

    Ok(())
}

pub fn check_attributes(
    xml_sdf: &XmlSystemDescription,
    node: &dyn SdfNode,
    attributes: &[&'static str],
) -> Result<(), String> {
    for attribute in node.attributes() {
        if !attributes.contains(&attribute.name) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("invalid attribute '{}'", attribute.name),
            ));
        }
    }

    Ok(())
}

pub fn checked_lookup<'a>(
    xml_sdf: &XmlSystemDescription,
    node: &'a dyn SdfNode,
    attribute: &'static str,
) -> Result<&'a str, String> {
    if let Some(value) = node.attribute(attribute) {
        Ok(value)
    } else {
        let pos = node.range().start;
        Err(format!(
            "Error: Missing required attribute '{}' on element '{}': {}:{}:{}",
            attribute,
            node.tag_name(),
            xml_sdf.filename.display(),
            pos.row,
            pos.col
        ))
    }
}

pub fn value_error(xml_sdf: &XmlSystemDescription, node: &dyn SdfNode, err: String) -> String {
    let pos = node.range().start;
    format!(
        "Error: {} on element '{}': {}:{}:{}",
        err,
        node.tag_name(),
        xml_sdf.filename.display(),
        pos.row,
        pos.col
    )
}

pub fn location_suffix_format(
    xml_sdf: &XmlSystemDescription,
    text_pos: Option<SdfLocation>,
) -> String {
    text_pos
        .map(|pos| format!("@ {}", loc_string(xml_sdf, pos)))
        .unwrap_or_default()
}
