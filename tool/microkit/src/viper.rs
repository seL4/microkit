//
// Copyright 2026, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause

use sel4_capdl_initializer_types::{Cap, Object};

use crate::capdl::CapDLSpecContainer;
use crate::sdf::SysMapPerms;
use crate::sdf::SystemDescription;

fn export_define_set(name: &'static str, vector: &[u64], target: &mut String) {
    if vector.is_empty() {
        target.push_str(&format!("define {name}(x) (false)\n"));
        target.push_str(&format!("define f_{name}(heap,gv,x) ({name}(x))\n"));
        return;
    }

    let items = vector
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");

    target.push_str(&format!("define {name}(x) (x in Set({items}))\n"));
    target.push_str(&format!("define f_{name}(heap,gv,x) ({name}(x))\n"));
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapView {
    pub occupied_slots: Vec<u64>,

    pub endpoint_caps: Vec<u64>,
    pub notification_caps: Vec<u64>,
    pub reply_caps: Vec<u64>,
    pub page_table_caps: Vec<u64>,
    pub irq_handler_caps: Vec<u64>,
    pub tcb_caps: Vec<u64>,
    pub vcpu_caps: Vec<u64>,
    pub ioport_caps: Vec<u64>,
    pub arm_smc_caps: Vec<u64>,
}

impl CapView {
    pub fn export(&self, target: &mut String) {
        export_define_set("pansel4_has_cap", &self.occupied_slots, target);
        export_define_set("pansel4_has_endpoint_cap", &self.endpoint_caps, target);
        export_define_set(
            "pansel4_has_notification_cap",
            &self.notification_caps,
            target,
        );
        export_define_set("pansel4_has_reply_cap", &self.reply_caps, target);
        export_define_set("pansel4_has_page_table_cap", &self.page_table_caps, target);
        export_define_set(
            "pansel4_has_irq_handler_cap",
            &self.irq_handler_caps,
            target,
        );
        export_define_set("pansel4_has_tcb_cap", &self.tcb_caps, target);
        export_define_set("pansel4_has_vcpu_cap", &self.vcpu_caps, target);
        export_define_set("pansel4_has_ioport_cap", &self.ioport_caps, target);
        export_define_set("pansel4_has_arm_smc_cap", &self.arm_smc_caps, target);
    }

    fn sort_and_dedup(&mut self) {
        fn go(v: &mut Vec<u64>) {
            v.sort_unstable();
            v.dedup();
        }
        go(&mut self.occupied_slots);
        go(&mut self.endpoint_caps);
        go(&mut self.notification_caps);
        go(&mut self.reply_caps);
        go(&mut self.page_table_caps);
        go(&mut self.irq_handler_caps);
        go(&mut self.tcb_caps);
        go(&mut self.vcpu_caps);
        go(&mut self.ioport_caps);
        go(&mut self.arm_smc_caps);
    }
}

pub fn get_cap_view(
    capdl_spec: &CapDLSpecContainer,
    system: &SystemDescription,
    current_pd: usize,
) -> Option<CapView> {
    let pd = system.protection_domains.get(current_pd)?;
    let cnode_name = format!("cnode_{}", pd.name);

    let named_obj = capdl_spec
        .spec
        .objects
        .iter()
        .find(|obj| obj.name.as_deref() == Some(cnode_name.as_str()))?;

    let Object::CNode(cnode) = &named_obj.object else {
        return None;
    };

    let mut view = CapView::default();

    for cte in &cnode.slots {
        let slot = u64::from(cte.slot.0);
        view.occupied_slots.push(slot);

        match &cte.cap {
            Cap::Endpoint(_) => {
                view.endpoint_caps.push(slot);
            }
            Cap::Notification(_) => {
                view.notification_caps.push(slot);
            }
            Cap::Reply(_) => {
                view.reply_caps.push(slot);
            }
            Cap::PageTable(_) => {
                view.page_table_caps.push(slot);
            }
            Cap::ArmIrqHandler(_)
            | Cap::IrqHandler(_)
            | Cap::IrqIOApicHandler(_)
            | Cap::IrqMsiHandler(_)
            | Cap::RiscvIrqHandler(_) => {
                view.irq_handler_caps.push(slot);
            }
            Cap::Tcb(_) => {
                view.tcb_caps.push(slot);
            }
            Cap::VCpu(_) => {
                view.vcpu_caps.push(slot);
            }
            Cap::IOPorts(_) => {
                view.ioport_caps.push(slot);
            }
            Cap::ArmSmc(_) => {
                view.arm_smc_caps.push(slot);
            }

            Cap::AsidPool(_)
            | Cap::CNode(_)
            | Cap::DomainSet(_)
            | Cap::Frame(_)
            | Cap::SchedContext(_)
            | Cap::IOSpace(_)
            | Cap::IOPageTable(_)
            | Cap::Untyped(_) => {
                /* ^ The caps above can occupy CSpace slots, but Viper
                 * verification currently has no use for them, so we
                 * intentionally do not emit anything here.
                 */
            }
        }
    }

    view.sort_and_dedup();
    Some(view)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SdfView {
    // the world, from the perspective of a single PD
    pub channel_ends: Vec<u64>,
    pub protected_sources: Vec<u64>,
    pub ppcall_targets: Vec<u64>,
    pub notified_sources: Vec<u64>,
    pub notify_targets: Vec<u64>,
    pub irqs: Vec<u64>,
    pub children: Vec<u64>,
}

impl SdfView {
    pub fn export(&self, target: &mut String) {
        export_define_set("mk_is_protected_source", &self.protected_sources, target);

        export_define_set("mk_is_ppcall_target", &self.ppcall_targets, target);

        export_define_set("mk_is_notified_source", &self.notified_sources, target);

        export_define_set("mk_is_notify_target", &self.notify_targets, target);

        export_define_set("mk_is_irq_channel", &self.irqs, target);

        export_define_set("mk_is_child", &self.children, target);
    }
}

pub fn get_sdf_view(system: &SystemDescription, current_pd: usize) -> Option<SdfView> {
    let current = system.protection_domains.get(current_pd)?;

    let mut view = SdfView {
        ..Default::default()
    };

    for irq in &current.irqs {
        view.notified_sources.push(irq.id);
        view.irqs.push(irq.id);
    }

    for ch in &system.channels {
        let (local, remote) = if ch.end_a.pd == current_pd {
            (&ch.end_a, &ch.end_b)
        } else if ch.end_b.pd == current_pd {
            (&ch.end_b, &ch.end_a)
        } else {
            continue;
        };

        view.channel_ends.push(local.id);

        let local_prio = current.priority();
        let remote_prio = system.protection_domains[remote.pd].priority();

        if local.pp && local_prio < remote_prio {
            view.ppcall_targets.push(local.id);
        }

        if remote.pp && remote_prio < local_prio {
            view.protected_sources.push(local.id);
        }

        if remote.notify {
            view.notified_sources.push(local.id);
        }

        if local.notify {
            view.notify_targets.push(local.id);
        }
    }

    for pd in &system.protection_domains {
        if pd.parent == Some(current_pd) {
            if let Some(id) = pd.id {
                view.children.push(id)
            }
        }
    }

    Some(view)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mem {
    pub name: String,
    pub start: u64,
    pub end: u64,
}

impl Mem {
    pub fn export(&self, target: &mut String) {
        let name: &String = &self.name;
        let start: u64 = self.start;
        let end: u64 = self.end;
        target.push_str(&format!(
            "define mem_{name}_contains(x) ({start} <= x && x < {end})\n"
        ));
    }
}

fn export_define_mem_set(perm: &'static str, vector: &[Mem], target: &mut String) {
    if vector.is_empty() {
        target.push_str(&format!("define mem_{perm}(x) (false)\n"));
        target.push_str(&format!("define f_mem_{perm}(heap,gv,x) (mem_{perm}(x))\n"));
        return;
    }
    let items = vector
        .iter()
        .map(|x| format!("mem_{}_contains(x)", x.name))
        .collect::<Vec<_>>()
        .join(" || ");

    target.push_str(&format!("define mem_{perm}(x) ({items})\n"));
    target.push_str(&format!("define f_mem_{perm}(heap,gv,x) (mem_{perm}(x))\n"));
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemView {
    pub read: Vec<Mem>,
    pub readwrite: Vec<Mem>,
}

impl MemView {
    pub fn export(&self, target: &mut String) {
        for mr in &self.read {
            mr.export(target);
        }

        export_define_mem_set("readable", &self.read, target);
        export_define_mem_set("writeable", &self.readwrite, target);
    }
}

pub fn get_mem_view(system: &SystemDescription, current_pd: usize) -> Option<MemView> {
    let current = system.protection_domains.get(current_pd)?;

    let mut view = MemView {
        ..Default::default()
    };

    for mr in &system.memory_regions {
        let mmaps_into_current_pd = current.maps.iter().filter(|map| map.mr == mr.name);

        for mmap in mmaps_into_current_pd {
            let start = mmap.vaddr;
            let end = start + mr.size;
            if end < start {
                // we catch bonkers mappings elsewhere, ignore them here!
                continue;
            }
            let name = mr.name.clone();
            let mem: Mem = Mem { name, start, end };
            view.read.push(mem.clone());

            let writeable = (mmap.perms & SysMapPerms::Write as u8) != 0;
            if writeable {
                view.readwrite.push(mem);
            }
        }
    }

    Some(view)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedView {
    pub pd_name: String,
    pub sdf: SdfView,
    pub cap: CapView,
    pub mem: MemView,
}

impl CombinedView {
    pub fn export(&self, target: &mut String) {
        self.sdf.export(target);
        self.cap.export(target);
        self.mem.export(target);
    }
}

pub fn get_combined_views(
    capdl_spec: &CapDLSpecContainer,
    system: &SystemDescription,
) -> Vec<CombinedView> {
    system
        .protection_domains
        .iter()
        .enumerate()
        .filter_map(|(current_pd, pd)| {
            let sdf = get_sdf_view(system, current_pd)?;
            let cap = get_cap_view(capdl_spec, system, current_pd)?;
            let mem = get_mem_view(system, current_pd)?;

            Some(CombinedView {
                pd_name: pd.name.clone(),
                sdf,
                cap,
                mem,
            })
        })
        .collect()
}
