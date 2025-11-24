//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

pub fn crc32(bytes: &[u8]) -> u32 {
    // Could be more optimised using the table approach.
    // Source: https://web.archive.org/web/20190108202303/http://www.hackersdelight.org/hdcodetxt/crc.c.txt
    let mut crc: u32 = 0xFFFF_FFFF;

    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }

    !crc
}
