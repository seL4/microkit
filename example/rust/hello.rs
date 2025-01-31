//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

#![no_std]
#![no_main]

use sel4_microkit::{NullHandler, protection_domain, debug_println};

#[protection_domain()]
fn init() -> NullHandler {
    debug_println!("hello, world from Rust!");

    NullHandler::new()
}
