#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from pathlib import Path
from struct import Struct, pack
from enum import IntEnum, IntFlag
from dataclasses import dataclass

from typing import List, Literal, Optional, Tuple


class ObjectFileType(IntEnum):
    ET_NONE = 0
    ET_REL = 1
    ET_EXEC = 2
    ET_DYN = 3
    ET_CORE = 4


class ObjectFileClass(IntEnum):
    ELFCLASS32 = 1
    ELFCLASS64 = 2


class DataEncoding(IntEnum):
    ELFDATA2LSB = 1
    ELFDATA2MSB = 2


class OperatingSystemAbi(IntEnum):
    ELFOSABI_SYSV = 0
    ELFOSABI_HPUX = 1
    ELFOSABI_STANDALINE = 255


class SegmentType(IntEnum):
    PT_NULL = 0
    PT_LOAD = 1
    PT_DYNAMIC = 2
    PT_INTERP = 3
    PT_NOTE = 4
    PT_SHLID = 5
    PT_PHDR = 6

class SegmentAttributes(IntFlag):
    PF_X = 0x1
    PF_W = 0x2
    PF_R = 0x4


class MachineType(IntEnum):
    # NOTE: Obviously there are may more!
    # This is all we support for now, and I don't
    # feel like typing them all out!
    # These values are from Linux source in include/uapi/linux/elf-em.h
    EM_AARCH64 = 183
    EM_RISCV = 243


class ElfVersion(IntEnum):
    EV_NONE = 0
    EV_CURRENT = 1


ELF_MAGIC = b'\x7FELF'
# Note: header struct excludes first 5 bytes (magic + class)
ELF_HEADER32 = Struct("<BBBBxxxxxxxHHIIIIIHHHHHH")
ELF_HEADER32_FIELDS = (
    "ident_data",
    "ident_version",
    "ident_osabi",
    "ident_abiversion",
    "type_",
    "machine",
    "version",
    "entry",
    "phoff",
    "shoff",
    "flags",
    "ehsize",
    "phentsize",
    "phnum",
    "shentsize",
    "shnum",
    "shstrndx",
)
ELF_PROGRAM_HEADER32 = Struct("<IIIIIIII")
ELF_PROGRAM_HEADER32_FIELDS = (
    "type_",
    "offset",
    "vaddr",
    "paddr",
    "filesz",
    "memsz",
    "flags",
    "align",
)
ELF_SECTION_HEADER32 = Struct("<IIIIIIIIII")
ELF_SECTION_HEADER32_FIELDS = (
    "name",
    "type_",
    "flags",
    "addr",
    "offset",
    "size",
    "link",
    "info",
    "addralign",
    "entsize",
)

ELF_HEADER64 = Struct("<BBBBxxxxxxxHHIQQQIHHHHHH")
ELF_HEADER64_FIELDS = (
    "ident_data",
    "ident_version",
    "ident_osabi",
    "ident_abiversion",
    "type_",
    "machine",
    "version",
    "entry",
    "phoff",
    "shoff",
    "flags",
    "ehsize",
    "phentsize",
    "phnum",
    "shentsize",
    "shnum",
    "shstrndx",
)
ELF_PROGRAM_HEADER64 = Struct("<IIQQQQQQ")
ELF_PROGRAM_HEADER64_FIELDS = (
    "type_",
    "flags",
    "offset",
    "vaddr",
    "paddr",
    "filesz",
    "memsz",
    "align",
)
ELF_SECTION_HEADER64 = Struct("<IIQQQQIIQQ")
ELF_SECTION_HEADER64_FIELDS = (
    "name",
    "type_",
    "flags",
    "addr",
    "offset",
    "size",
    "link",
    "info",
    "addralign",
    "entsize",
)
ELF_SYMBOL64 = Struct("<IBBHQQ")
ELF_SYMBOL64_FIELDS = (
    "name",
    "info",
    "other",
    "shndx",
    "value",
    "size",
)

class InvalidElf(Exception):
    pass


@dataclass
class ElfHeader:
    ident_data: int
    ident_version: int
    ident_osabi: int
    ident_abiversion: int
    type_: int
    machine: int
    version: int
    entry: int
    phoff: int
    shoff: int
    flags: int
    ehsize: int
    phentsize: int
    phnum: int
    shentsize: int
    shnum: int
    shstrndx: int


@dataclass
class ElfProgramHeader:
    type_: int
    offset: int
    vaddr: int
    paddr: int
    filesz: int
    memsz: int
    flags: int
    align: int


@dataclass
class ElfSectionHeader:
    name: int
    type_: int
    flags: int
    addr: int
    offset: int
    size: int
    link: int
    info: int
    addralign: int
    entsize: int


@dataclass
class ElfSymbol:
    name: int
    info: int
    other: int
    shndx: int
    value: int
    size: int


class ElfSegment:
    def __init__(self, phys_addr: int, virt_addr: int, data: bytearray, loadable: bool, attrs: SegmentAttributes) -> None:
        self.data = data
        self.phys_addr = phys_addr
        self.virt_addr = virt_addr
        self.loadable = loadable
        self.attrs = attrs

    def __repr__(self) -> str:
        return f"<ElfSegment phys_addr=0x{self.phys_addr:x} virt_addr=0x{self.virt_addr:x} mem_size={self.mem_size}>"

    # FIXME: Is this really useful?
    @property
    def mem_size(self) -> int:
        return len(self.data)

    @property
    def is_writable(self) -> bool:
        return (self.attrs & SegmentAttributes.PF_W) != 0

    @property
    def is_readable(self) -> bool:
        return (self.attrs & SegmentAttributes.PF_R) != 0

    @property
    def is_executable(self) -> bool:
        return (self.attrs & SegmentAttributes.PF_X) != 0


class ElfFile:
    def __init__(self, word_size: Literal[32, 64] = 64) -> None:
        self.segments: List[ElfSegment] = []
        self._symbols: List[Tuple[str, ElfSymbol]] = []
        self.word_size = word_size
        self.entry: int = 0x0

    @classmethod
    def from_path(cls, path: Path) -> "ElfFile":
        with path.open("rb") as f:
            magic = f.read(4)
            if magic != ELF_MAGIC:
                raise InvalidElf("Incorrect magic")
            class_ = f.read(1)[0]
            if class_ == 1:
                hdr_fmt = ELF_HEADER32
                hdr_fields = ELF_HEADER32_FIELDS
                ph_fmt = ELF_PROGRAM_HEADER32
                ph_fields = ELF_PROGRAM_HEADER32_FIELDS
                sh_fmt = ELF_SECTION_HEADER32
                sh_fields = ELF_SECTION_HEADER32_FIELDS
                elf = cls(word_size=32)
            elif class_ == 2:
                hdr_fmt = ELF_HEADER64
                hdr_fields = ELF_HEADER64_FIELDS
                ph_fmt = ELF_PROGRAM_HEADER64
                ph_fields = ELF_PROGRAM_HEADER64_FIELDS
                sh_fmt = ELF_SECTION_HEADER64
                sh_fields = ELF_SECTION_HEADER64_FIELDS
                sym_fmt = ELF_SYMBOL64
                sym_fields = ELF_SYMBOL64_FIELDS
                elf = cls(word_size=64)
            else:
                raise InvalidElf(f"Invalid class '{class_}'")

            hdr_raw = f.read(hdr_fmt.size)
            hdr: ElfHeader = ElfHeader(**dict(zip(hdr_fields, hdr_fmt.unpack(hdr_raw))))
            elf.entry = hdr.entry

            f.seek(hdr.phoff)
            for idx in range(hdr.phnum):
                f.seek(hdr.phoff + idx * hdr.phentsize)
                phent_raw = f.read(hdr.phentsize)
                phent = ElfProgramHeader(**dict(zip(ph_fields, ph_fmt.unpack_from(phent_raw))))
                f.seek(phent.offset)
                data = f.read(phent.filesz)
                zeros = bytes(phent.memsz - phent.filesz)
                elf.segments.append(ElfSegment(phent.paddr, phent.vaddr, bytearray(data + zeros), phent.type_ == 1, SegmentAttributes(phent.flags)))


            # FIXME: Add support for sections and symbols
            f.seek(hdr.shoff)
            shents = []
            symtab_shent: Optional[ElfSectionHeader] = None
            for idx in range(hdr.shnum):
                shent_raw = f.read(hdr.shentsize)
                shent = ElfSectionHeader(**dict(zip(sh_fields, sh_fmt.unpack_from(shent_raw))))
                shents.append(shent)
                if shent.type_ == 3:
                    shstrtab_shent = shent
                if shent.type_ == 2:
                    assert symtab_shent is None
                    symtab_shent = shent

            if shstrtab_shent is None:
                raise InvalidElf("Unable to find string table section")

            f.seek(shstrtab_shent.offset)

            # Microkit requires the symbol table to exist
            assert symtab_shent is not None, f"The symbol table for the given ELF '{path}' could not be found"
            f.seek(symtab_shent.offset)
            _symtab = f.read(symtab_shent.size)

            symtab_str = shents[symtab_shent.link]
            f.seek(symtab_str.offset)
            _symtab_str = f.read(symtab_str.size)

            offset = 0
            elf._symbols = []
            while offset < len(_symtab):
                sym = ElfSymbol(**dict(zip(sym_fields, sym_fmt.unpack_from(_symtab, offset))))
                name = cls._get_string(_symtab_str, sym.name)
                offset += sym_fmt.size
                elf._symbols.append((name, sym))

        return elf

    def write(self, path: Path, machine: MachineType) -> None:
        """Note: This only supports writing out of program headers
        and segments. It does *not* support writing out sections
        at this point in time.
        """
        with path.open("wb") as f:
            ehsize = ELF_HEADER64.size + 5
            phentsize = ELF_PROGRAM_HEADER64.size
            header = ElfHeader(
                ident_data=DataEncoding.ELFDATA2LSB,
                ident_version=ElfVersion.EV_CURRENT,
                ident_osabi=OperatingSystemAbi.ELFOSABI_STANDALINE,
                ident_abiversion=0,
                type_ = ObjectFileType.ET_EXEC,
                machine=machine,
                version=ElfVersion.EV_CURRENT,
                entry=self.entry,
                phoff=ehsize,
                shoff=0,
                flags=0,
                ehsize=ehsize,
                phentsize=phentsize,
                phnum=len(self.segments),
                shentsize=0,
                shnum=0,
                shstrndx=0,
            )
            header_bytes = ELF_HEADER64.pack(*(getattr(header, field) for field in ELF_HEADER64_FIELDS))
            f.write(ELF_MAGIC)
            f.write(pack("<B", ObjectFileClass.ELFCLASS64))
            f.write(header_bytes)

            data_offset = ehsize + len(self.segments) * phentsize
            for segment in self.segments:
                pheader = ElfProgramHeader(
                    type_ = SegmentType.PT_LOAD,
                    offset = data_offset,
                    vaddr = segment.virt_addr,
                    paddr = segment.phys_addr,
                    filesz = segment.mem_size,
                    memsz = segment.mem_size,
                    # FIXME: Need to do something better with permissions in the future!
                    flags = SegmentAttributes.PF_R | SegmentAttributes.PF_W | SegmentAttributes.PF_X,
                    align = 1,
                )
                pheader_bytes = ELF_PROGRAM_HEADER64.pack(*(getattr(pheader, field) for field in ELF_PROGRAM_HEADER64_FIELDS))
                f.write(pheader_bytes)
                data_offset += len(segment.data)

            for segment in self.segments:
                f.write(segment.data)


    def add_segment(self, segment: ElfSegment) -> None:
        # TODO: Check that the segment doesn't overlap any existing
        # segment
        # TODO: We may want to keep segments in order.
        self.segments.append(segment)


    def get_data(self, vaddr: int, size: int) -> bytes:
        for seg in self.segments:
            if vaddr >= seg.virt_addr and vaddr + size <= seg.virt_addr + len(seg.data):
                offset = vaddr - seg.virt_addr
                return seg.data[offset:offset+size]

        raise Exception(f"Unable to find data for vaddr=0x{vaddr:x} size=0x{size:x}")

    def write_symbol(self, variable_name: str, data: bytes) -> None:
        vaddr, size = self.find_symbol(variable_name)
        for seg in self.segments:
            if vaddr >= seg.virt_addr and vaddr + size <= seg.virt_addr + len(seg.data):
                offset = vaddr - seg.virt_addr
                assert len(data) <= size
                seg.data[offset:offset+len(data)] = data


    # def read(self, offset: int, size: int) -> bytes:
    #     self._f.seek(offset)
    #     return self._f.read(size)

    # def _get_sh_string(self, idx: int) -> str:
    #     end_idx = self._shstrtab.find(0, idx)
    #     return self._shstrtab[idx:end_idx].decode("ascii")

    @staticmethod
    def _get_string(strtab: bytes, idx: int) -> str:
        end_idx = strtab.find(0, idx)
        return strtab[idx:end_idx].decode("utf8")

    def find_symbol(self, variable_name: str) -> Tuple[int, int]:
        found_sym = self.find_symbol_if_exists(variable_name)
        if found_sym is None:
            raise KeyError(f"No symbol named {variable_name} found")

        return found_sym

    def find_symbol_if_exists(self, variable_name: str) -> Optional[Tuple[int, int]]:
        found_sym = None
        for name, sym in self._symbols:
            if name == variable_name:
                if found_sym is None:
                    found_sym = sym
                else:
                    raise Exception(f"Multiple symbols with name {variable_name}")
        if found_sym is None:
            return None
        # symbol_type = found_sym.info & 0xf
        # symbol_binding = found_sym.info >> 4
        #if symbol_type != 1:
        #    raise Exception(f"Unexpected symbol type {symbol_type}. Expect STT_OBJECT")
        return found_sym.value, found_sym.size

    def read_struct(self, variable_name: str, struct_: Struct) -> Tuple[int, ...]:
        vaddr, size = self.find_symbol(variable_name)
        return struct_.unpack_from(self.get_data(vaddr, size))

