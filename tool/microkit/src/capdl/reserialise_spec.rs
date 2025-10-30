//
// Copyright 2023, Colias Group, LLC
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// A simple reimplementation of
// https://github.com/seL4/rust-sel4/blob/6f8d1baaad3aaca6f20966a2acb40e4651546519/crates/sel4-capdl-initializer/add-spec/src/reserialize_spec.rs
// We can't reuse the original code because it assumes that we are loading ELF frames from files.
// Which isn't suitable for Microkit as we want to embed the frames' data directly into the spec for
// easily patching ELF symbols.

use std::ops::Range;

use sel4_capdl_initializer_types::*;

use crate::{capdl::spec::ElfContent, elf::ElfFile};

// @billn TODO: instead of doing this serialise our type -> deserialise into their type -> serialise business
//              we can directly insert IndirectObjectName and IndirectDeflatedBytesContent into our spec type
//              and one shot serialise at the cost of more complicated type definitions in spec.rs.
//              But this is more of a performance concern rather than a bug.

// Given a `Spec` data structure from sel4_capdl_initializer_types, "flatten" it into a vector of bytes
// for encapsulating it into the CapDL initialiser ELF.
pub fn reserialise_spec(
    elfs: &[ElfFile],
    input_spec: &Spec<'static, String, ElfContent, ()>,
    object_names_level: &ObjectNamesLevel,
) -> Vec<u8> {
    // A data structure to manage allocation of buffers in the flattened spec.
    let mut sources = SourcesBuilder::new();

    let final_spec = input_spec
        // This first step applies the debugging level from `object_names_level` to all root object
        // and copy them into `sources`.
        .traverse_names_with_context(|named_obj| {
            object_names_level
                .apply(named_obj)
                .map(|s| IndirectObjectName {
                    range: sources.append(s.as_bytes()),
                })
        })
        // The final step is to take the frame data and compress it using miniz_oxide::deflate::compress_to_vec()
        // to save memory then append it to `sources`.
        .traverse_data(|data| IndirectDeflatedBytesContent {
            deflated_bytes_range: sources.append(&DeflatedBytesContent::pack(
                &elfs
                    .get(data.elf_id)
                    .unwrap()
                    .segments
                    .get(data.elf_seg_idx)
                    .unwrap()
                    .data()[data.elf_seg_data_range.clone()],
            )),
        });

    let mut blob = postcard::to_allocvec(&final_spec).unwrap();
    blob.extend(sources.build());
    blob
}

struct SourcesBuilder {
    buf: Vec<u8>,
}

impl SourcesBuilder {
    fn new() -> Self {
        Self { buf: vec![] }
    }

    fn build(self) -> Vec<u8> {
        self.buf
    }

    fn append(&mut self, bytes: &[u8]) -> Range<usize> {
        let start = self.buf.len();
        self.buf.extend(bytes);
        let end = self.buf.len();
        start..end
    }
}
