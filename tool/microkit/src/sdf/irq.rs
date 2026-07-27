//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use super::pci::PciDevice;

use crate::sel4::{ArmRiscvIrqTrigger, X86IoapicIrqPolarity, X86IoapicIrqTrigger};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, Eq)]
pub enum SysIrqKind {
    Conventional {
        irq: u64,
        trigger: ArmRiscvIrqTrigger,
    },
    /// x86-64 specific
    IOAPIC {
        ioapic: u64,
        pin: u64,
        trigger: X86IoapicIrqTrigger,
        polarity: X86IoapicIrqPolarity,
        vector: u64,
    },
    /// x86-64 specific
    MSI {
        pci_device: PciDevice,
        handle: u64,
        vector: u64,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SysIrq {
    pub id: u64,
    pub kind: SysIrqKind,
}

impl SysIrq {
    pub fn irq_num(&self) -> u64 {
        match self.kind {
            SysIrqKind::Conventional { irq, .. } => irq,
            SysIrqKind::IOAPIC { vector, .. } => vector,
            SysIrqKind::MSI { vector, .. } => vector,
        }
    }
}
