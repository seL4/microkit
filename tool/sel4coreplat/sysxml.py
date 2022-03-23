#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from dataclasses import dataclass, replace
from pathlib import Path
# See: https://stackoverflow.com/questions/6949395/is-there-a-way-to-get-a-line-number-from-an-elementtree-element
# Force use of Python elementtree to avoid overloading
import sys
sys.modules['_elementtree'] = None  # type: ignore
import xml.etree.ElementTree as ET

from typing import Dict, Iterable, Optional, Set, Tuple

from sel4coreplat.util import str_to_bool, UserError

MIN_PAGE_SIZE = 0x1000 # FIXME: This shouldn't be here


class MissingAttribute(Exception):
    def __init__(self, attribute_name: str, element: ET.Element):
        super().__init__(f"Missing attribute: {attribute_name}")
        self.attribute_name = attribute_name
        self.element = element


def checked_lookup(el: ET.Element, attr: str) -> str:
    try:
        return el.attrib[attr]
    except KeyError:
        raise MissingAttribute(attr, el)


def _check_attrs(el: ET.Element, valid_keys: Iterable[str]) -> None:
    for key in el.attrib:
        if key not in valid_keys:
            raise ValueError(f"invalid attribute '{key}'")


@dataclass(frozen=True, eq=True)
class PlatformDescription:
    page_sizes: Tuple[int, ...]


class LineNumberingParser(ET.XMLParser):
    def __init__(self, path: Path):
        super().__init__()
        self._path = path

    def _start(self, *args, **kwargs):  # type: ignore
        element = super(self.__class__, self)._start(*args, **kwargs)
        element._path = self._path
        element._start_line_number = self.parser.CurrentLineNumber
        element._start_column_number = self.parser.CurrentColumnNumber
        element._loc_str = f"{element._path}:{element._start_line_number}.{element._start_column_number}"
        return element


@dataclass(frozen=True, eq=True)
class SysMap:
    mr: str
    vaddr: int
    perms: str  # FIXME: should make this a better typed thing
    cached: bool
    element: Optional[ET.Element]


@dataclass(frozen=True, eq=True)
class SysIrq:
    irq: int
    id_: int


@dataclass(frozen=True, eq=True)
class SysSetVar:
    symbol: str
    region_paddr: Optional[str] = None
    vaddr: Optional[int] = None


@dataclass(frozen=True, eq=True)
class ProtectionDomain:
    pd_id: Optional[int]
    name: str
    priority: int
    budget: int
    period: int
    pp: bool
    program_image: Path
    maps: Tuple[SysMap, ...]
    irqs: Tuple[SysIrq, ...]
    setvars: Tuple[SysSetVar, ...]
    child_pds: Tuple["ProtectionDomain", ...]
    parent: Optional["ProtectionDomain"]
    element: ET.Element


@dataclass(frozen=True, eq=True)
class SysMemoryRegion:
    name: str
    size: int
    page_size: int
    page_count: int
    phys_addr: Optional[int]


@dataclass(frozen=True, eq=True)
class Channel:
    pd_a: str
    id_a: int
    pd_b: str
    id_b: int
    element: ET.Element


def _pd_tree_to_list(root_pd: ProtectionDomain, parent_pd: Optional[ProtectionDomain]) -> Tuple[ProtectionDomain, ...]:
    # Check child PDs have unique identifiers
    child_ids = set()
    for child_pd in root_pd.child_pds:
        if child_pd.pd_id in child_ids:
            raise UserError(f"duplicate pd_id: {child_pd.pd_id} in protection domain: '{root_pd.name}' @ {child_pd.element._loc_str}")  # type: ignore
        child_ids.add(child_pd.pd_id)

    new_root_pd = replace(root_pd, child_pds=tuple(), parent=parent_pd)
    new_child_pds = sum((_pd_tree_to_list(child_pd, new_root_pd) for child_pd in root_pd.child_pds), tuple())
    return (new_root_pd, ) + new_child_pds


def _pd_flatten(pds: Iterable[ProtectionDomain]) -> Tuple[ProtectionDomain, ...]:
    """Given an iterable of protection domains flatten the tree representation
    into a flat tuple.

    In doing so the representation is changed from "Node with list of children",
    to each node having a parent link instead.
    """
    return sum((_pd_tree_to_list(pd, None) for pd in pds), tuple())


class SystemDescription:
    def __init__(
        self,
        memory_regions: Iterable[SysMemoryRegion],
        protection_domains: Iterable[ProtectionDomain],
        channels: Iterable[Channel]
    ) -> None:
        self.memory_regions = tuple(memory_regions)
        self.protection_domains = _pd_flatten(protection_domains)
        self.channels = tuple(channels)

        # Note: These could be dict comprehensions, but
        # we want to perform duplicate checks as we
        # build the data structure
        self.pd_by_name: Dict[str, ProtectionDomain] = {}
        self.mr_by_name: Dict[str, SysMemoryRegion] = {}

        # Ensure there is at least one protection domain
        if len(self.protection_domains) == 0:
            raise UserError("At least one protection domain must be defined")

        if len(self.protection_domains) > 63:
            raise UserError(f"Too many protection domains ({len(self.protection_domains)}) defined. Maximum is 63.")

        for pd in protection_domains:
            if pd.name in self.pd_by_name:
                raise UserError(f"Duplicate protection domain name '{pd.name}'.")
            self.pd_by_name[pd.name] = pd

        for mr in memory_regions:
            if mr.name in self.mr_by_name:
                raise UserError(f"Duplicate memory region name '{mr.name}'.")
            self.mr_by_name[mr.name] = mr

        # Ensure all CCs make senses
        for cc in self.channels:
            for pd_name in (cc.pd_a, cc.pd_b):
                if pd_name not in self.pd_by_name:
                    raise UserError(f"Invalid pd name '{pd_name}'. on element '{cc.element.tag}': {cc.element._loc_str}")  # type: ignore

        # Ensure no duplicate IRQs
        all_irqs = set()
        for pd in self.protection_domains:
            for sysirq in pd.irqs:
                if sysirq.irq in all_irqs:
                    raise UserError(f"duplicate irq: {sysirq.irq} in protection domain: '{pd.name}' @ {pd.element._loc_str}")  # type: ignore
                all_irqs.add(sysirq.irq)

        # Ensure no duplicate channel identifiers
        ch_ids: Dict[str, Set[int]] = {pd_name: set() for pd_name in self.pd_by_name}
        for pd in self.protection_domains:
            for sysirq in pd.irqs:
                if sysirq.id_ in ch_ids[pd.name]:
                    raise UserError(f"duplicate channel id: {sysirq.id_} in protection domain: '{pd.name}' @ {pd.element._loc_str}")  # type: ignore
                ch_ids[pd.name].add(sysirq.id_)

        for cc in self.channels:
            if cc.id_a in ch_ids[cc.pd_a]:
                pd = self.pd_by_name[cc.pd_a]
                raise UserError(f"duplicate channel id: {cc.id_a} in protection domain: '{pd.name}' @ {pd.element._loc_str}")  # type: ignore

            if cc.id_b in ch_ids[cc.pd_b]:
                pd = self.pd_by_name[cc.pd_b]
                raise UserError(f"duplicate channel id: {cc.id_b} in protection domain: '{pd.name}' @ {pd.element._loc_str}")  # type: ignore

            ch_ids[cc.pd_a].add(cc.id_a)
            ch_ids[cc.pd_b].add(cc.id_b)

        # Ensure that all maps are correct
        for pd in self.protection_domains:
            for map in pd.maps:
                if map.mr not in self.mr_by_name:
                    raise UserError(f"Invalid memory region name '{map.mr}' on '{map.element.tag}' @ {map.element._loc_str}")  # type: ignore

                mr = self.mr_by_name[map.mr]
                extra = map.vaddr % mr.page_size
                if extra != 0:
                    raise UserError(f"Invalid vaddr alignment on '{map.element.tag}' @ {map.element._loc_str}")  # type: ignore


        # Note: Overlapping memory is checked in the build.

        # Ensure all memory regions are used at least once. This only generates
        # warnings, not errors
        check_mrs = set(self.mr_by_name.keys())
        for pd in self.protection_domains:
            for m in pd.maps:
                if m.mr in check_mrs:
                    check_mrs.remove(m.mr)

        for mr_ in check_mrs:
            print(f"WARNING: Unused memory region: {mr_}")


def xml2mr(mr_xml: ET.Element, plat_desc: PlatformDescription) -> SysMemoryRegion:
    _check_attrs(mr_xml, ("name", "size", "page_size", "phys_addr"))
    name = checked_lookup(mr_xml, "name")
    size = int(checked_lookup(mr_xml, "size"), base=0)
    page_size_str = mr_xml.attrib.get("page_size")
    page_size = min(plat_desc.page_sizes) if page_size_str is None else int(page_size_str, base=0)
    if page_size not in plat_desc.page_sizes:
        raise ValueError(f"page size 0x{page_size:x} not supported")
    if size % page_size != 0:
        raise ValueError("size is not a multiple of the page size")
    paddr_str = mr_xml.attrib.get("phys_addr")
    paddr = None if paddr_str is None else int(paddr_str, base=0)
    if paddr is not None and paddr % page_size != 0:
        raise ValueError("phys_addr is not aligned to the page size")
    page_count = size // page_size
    return SysMemoryRegion(name, size, page_size, page_count, paddr)


def xml2pd(pd_xml: ET.Element, is_child: bool=False) -> ProtectionDomain:
    root_attrs = ("name", "priority", "pp", "budget", "period")
    child_attrs = root_attrs + ("pd_id", )
    _check_attrs(pd_xml, child_attrs if is_child else root_attrs)
    program_image: Optional[Path] = None
    name = checked_lookup(pd_xml, "name")
    priority = int(pd_xml.attrib.get("priority", "0"), base=0)
    if priority < 0 or priority > 254:
        raise ValueError("priority must be between 0 and 254")

    budget = int(pd_xml.attrib.get("budget", "1000"), base=0)
    period = int(pd_xml.attrib.get("period", str(budget)), base=0)
    pd_id = None
    if is_child:
        pd_id = int(checked_lookup(pd_xml, "pd_id"), base=0)
        if pd_id < 0 or pd_id > 255:
            raise ValueError("pd_id must be between 0 and 255")
    else:
        pd_id = None

    if budget > period:
        raise ValueError(f"budget ({budget}) must be less than, or equal to, period ({period})")

    pp = str_to_bool(pd_xml.attrib.get("pp", "false"))

    maps = []
    irqs = []
    setvars = []
    child_pds = []
    for child in pd_xml:
        try:
            if child.tag == "program_image":
                _check_attrs(child, ("path", ))
                if program_image is not None:
                    raise ValueError("program_image must only be specified once")
                program_image = Path(checked_lookup(child, "path"))
            elif child.tag == "map":
                _check_attrs(child, ("mr", "vaddr", "perms", "cached", "setvar_vaddr"))
                mr = checked_lookup(child, "mr")
                vaddr = int(checked_lookup(child, "vaddr"), base=0)
                perms = child.attrib.get("perms", "rw")
                cached = str_to_bool(child.attrib.get("cached", "true"))
                maps.append(SysMap(mr, vaddr, perms, cached, child))

                setvar_vaddr = child.attrib.get("setvar_vaddr")
                if setvar_vaddr:
                    setvars.append(SysSetVar(setvar_vaddr, vaddr=vaddr))
            elif child.tag == "irq":
                _check_attrs(child, ("irq", "id"))
                irq = int(checked_lookup(child, "irq"), base=0)
                id_ = int(checked_lookup(child, "id"), base=0)
                irqs.append(SysIrq(irq, id_))
            elif child.tag == "setvar":
                _check_attrs(child, ("symbol", "region_paddr"))
                symbol = checked_lookup(child, "symbol")
                region_paddr = checked_lookup(child, "region_paddr")
                setvars.append(SysSetVar(symbol, region_paddr=region_paddr))
            elif child.tag == "protection_domain":
                child_pds.append(xml2pd(child, is_child=True))
            else:
                raise UserError(f"Invalid XML element '{child.tag}': {child._loc_str}")  # type: ignore
        except ValueError as e:
            raise UserError(f"Error: {e} on element '{child.tag}': {child._loc_str}")  # type: ignore

    if program_image is None:
        raise ValueError("program_image must be specified")

    return ProtectionDomain(
        pd_id,
        name,
        priority,
        budget,
        period,
        pp,
        program_image,
        tuple(maps),
        tuple(irqs),
        tuple(setvars),
        tuple(child_pds),
        None,
        pd_xml
    )


def xml2channel(ch_xml: ET.Element) -> Channel:
    _check_attrs(ch_xml, ())
    ends = []
    for child in ch_xml:
        try:
            if child.tag == "end":
                _check_attrs(ch_xml, ("pd", "id"))
                pd = checked_lookup(child, "pd")
                id_ = int(checked_lookup(child, "id"))
                if id_ >= 64:
                    raise ValueError("id must be < 64")
                if id_ < 0:
                    raise ValueError("id must be >= 0")
                ends.append((pd, id_))
            else:
                raise UserError(f"Invalid XML element '{child.tag}': {child._loc_str}")  # type: ignore
        except ValueError as e:
            raise UserError(f"Error: {e} on element '{child.tag}': {child._loc_str}")  # type: ignore

    if len(ends) != 2:
        raise ValueError("exactly two end elements must be specified")

    return Channel(ends[0][0], ends[0][1], ends[1][0], ends[1][1], ch_xml)



def _check_no_text(el: ET.Element) -> None:
    if not (el.text is None or el.text.strip() == ""):
        raise UserError(f"Error: unexpected text found in element '{el.tag}' @ {el._loc_str}")  # type: ignore
    if not (el.tail is None or el.tail.strip() == ""):
        raise UserError(f"Error: unexpected text found after element '{el.tag}' @ {el._loc_str}")  # type: ignore
    for child in el:
        _check_no_text(child)


def xml2system(filename: Path, plat_desc: PlatformDescription) -> SystemDescription:
    try:
        tree = ET.parse(filename, parser=LineNumberingParser(filename))
    except ET.ParseError as e:
        line, column = e.position
        raise UserError(f"XML parse error: {filename}:{line}.{column}")

    root = tree.getroot()
    memory_regions = []
    protection_domains = []
    channels = []

    # Ensure there is no non-whitespace text
    _check_no_text(root)

    for child in root:
        try:
            if child.tag == "memory_region":
                memory_regions.append(xml2mr(child, plat_desc))
            elif child.tag == "protection_domain":
                protection_domains.append(xml2pd(child))
            elif child.tag == "channel":
                channels.append(xml2channel(child))
            else:
                raise UserError(f"Invalid XML element '{child.tag}': {child._loc_str}")  # type: ignore
        except ValueError as e:
            raise UserError(f"Error: {e} on element '{child.tag}': {child._loc_str}")  # type: ignore
        except MissingAttribute as e:
            raise UserError(f"Error: Missing required attribute '{e.attribute_name}' on element '{e.element.tag}': {e.element._loc_str}")  # type: ignore

    return SystemDescription(
        memory_regions=memory_regions,
        protection_domains=protection_domains,
        channels=channels,
    )
