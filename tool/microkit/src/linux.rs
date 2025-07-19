#[repr(C)]
struct linux_image_header_arm64 {
    code0: u32,       // Executable code
    code1: u32,       // Executable code
    text_offset: u64, // Image load offset, little endian
    image_size: u64,  // Effective Image size, little endian
    flags: u64,       // kernel flags, little endian
    res2: u64,        // reserved
    res3: u64,        // reserved
    res4: u64,        // reserved
    magic: u32,       // Magic number, little endian, "ARM\x64"
    res5: u32,        // reserved (used for PE COFF offset)
};
