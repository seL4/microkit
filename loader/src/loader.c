/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <stddef.h>

_Static_assert(sizeof(uintptr_t) == 8 || sizeof(uintptr_t) == 4, "Expect uintptr_t to be 32-bit or 64-bit");

#if UINTPTR_MAX == 0xffffffffUL
#define WORD_SIZE 32
#else
#define WORD_SIZE 64
#endif

#if WORD_SIZE == 32
#define MAGIC 0x5e14dead
#else
#define MAGIC 0x5e14dead14de5ead
#endif

#define ALIGN(n)  __attribute__((__aligned__(n)))

#define MASK(x) ((1U << x) - 1)

#define STACK_SIZE 4096

#define UART_REG(x) ((volatile uint32_t *)(UART_BASE + (x)))

#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2)
#define GICD_BASE 0x00F9010000UL
#define GICC_BASE 0x00F9020000UL
#elif defined(BOARD_qemu_virt_aarch64)
#define GICD_BASE 0x8000000UL
#define GICC_BASE 0x8010000UL
#endif

#define REGION_TYPE_DATA 1
#define REGION_TYPE_ZERO 2

#define FLAG_SEL4_HYP (1UL << 0)

enum el {
    EL0 = 0,
    EL1 = 1,
    EL2 = 2,
    EL3 = 3,
};

struct region {
    uintptr_t load_addr;
    uintptr_t size;
    uintptr_t offset;
    uintptr_t type;
};

struct loader_data {
    uintptr_t magic;
    uintptr_t size;
    uintptr_t flags;
    uintptr_t kernel_entry;
    uintptr_t ui_p_reg_start;
    uintptr_t ui_p_reg_end;
    uintptr_t pv_offset;
    uintptr_t v_entry;

    uintptr_t num_regions;
    struct region regions[];
};

typedef void (*sel4_entry)(
    uintptr_t ui_p_reg_start,
    uintptr_t ui_p_reg_end,
    intptr_t pv_offset,
    uintptr_t v_entry,
    uintptr_t dtb_addr_p,
    uintptr_t dtb_size,
    uintptr_t extra_device_addr_p,
    uintptr_t extra_device_size
);

static void *memcpy(void *dst, const void *src, size_t sz)
{
    char *dst_ = dst;
    const char *src_ = src;
    while (sz-- > 0) {
        *dst_++ = *src_++;
    }

    return dst;
}

void *memmove(void *restrict dest, const void *restrict src, size_t n)
{
    unsigned char *d = (unsigned char *)dest;
    const unsigned char *s = (const unsigned char *)src;

    /* no copying to do */
    if (d == s) {
        return dest;
    }
    /* for non-overlapping regions, just use memcpy */
    else if (s + n <= d || d + n <= s) {
        return memcpy(dest, src, n);
    }
    /* if copying from the start of s to the start of d, just use memcpy */
    else if (s > d) {
        return memcpy(dest, src, n);
    }

    /* copy from end of 's' to end of 'd' */
    size_t i;
    for (i = 1; i <= n; i++) {
        d[n - i] = s[n - i];
    }

    return dest;
}

char _stack[STACK_SIZE] ALIGN(16);

#ifdef ARCH_aarch64
void switch_to_el1(void);
void switch_to_el2(void);
void el1_mmu_enable(void);
void el2_mmu_enable(void);
extern char arm_vector_table[1];

/* Paging structures for kernel mapping */
uint64_t boot_lvl0_upper[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl1_upper[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl2_upper[1 << 9] ALIGN(1 << 12);

/* Paging structures for identity mapping */
uint64_t boot_lvl0_lower[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl1_lower[1 << 9] ALIGN(1 << 12);

uintptr_t exception_register_state[32];
#elif defined(ARCH_riscv64)
/* Paging structures for kernel mapping */
uint64_t boot_lvl1_pt[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl2_pt[1 << 9] ALIGN(1 << 12);
/* Paging structures for identity mapping */
uint64_t boot_lvl2_pt_elf[1 << 9] ALIGN(1 << 12);
#endif

extern char _text;
extern char _bss_end;
const struct loader_data *loader_data = (void *) &_bss_end;

#if defined(BOARD_tqma8xqp1gb)
#define UART_BASE 0x5a070000
#define STAT 0x14
#define TRANSMIT 0x1c
#define STAT_TDRE (1 << 23)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}

#elif defined(BOARD_imx8mm_evk) || defined(BOARD_imx8mp_evk) || defined(BOARD_imx8mp_iotgate)
#define UART_BASE 0x30890000
#define STAT 0x98
#define TRANSMIT 0x40
#define STAT_TDRE (1 << 14)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}
#elif defined(BOARD_zcu102)
#define UART_BASE 0xff000000
#define UART_CHANNEL_STS_TXEMPTY 0x8
#define UART_CHANNEL_STS         0x2C
#define UART_TX_RX_FIFO          0x30

#define UART_CR             0x00
#define UART_CR_TX_EN       (1 << 4)
#define UART_CR_TX_DIS      (1 << 5)

static void uart_init(void)
{
    uint32_t ctrl = *UART_REG(UART_CR);
    ctrl |= UART_CR_TX_EN;
    ctrl &= ~UART_CR_TX_DIS;
    *UART_REG(UART_CR) = ctrl;
}

static void putc(uint8_t ch)
{
    while (!(*UART_REG(UART_CHANNEL_STS) & UART_CHANNEL_STS_TXEMPTY));
    *UART_REG(UART_TX_RX_FIFO) = ch;
}
#elif defined(BOARD_maaxboard) || defined(BOARD_imx8mq_evk)
#define UART_BASE 0x30860000
#define STAT 0x98
#define TRANSMIT 0x40
#define STAT_TDRE (1 << 14)

static void uart_init() {}

static void putc(uint8_t ch)
{
    // ensure FIFO has space
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}
#elif defined(BOARD_odroidc2)
#define UART_BASE 0xc81004c0
#define UART_WFIFO 0x0
#define UART_STATUS 0xC
#define UART_TX_FULL (1 << 21)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while ((*UART_REG(UART_STATUS) & UART_TX_FULL));
    *UART_REG(UART_WFIFO) = ch;
}
#elif defined(BOARD_odroidc4)
#define UART_BASE 0xff803000
#define UART_WFIFO 0x0
#define UART_STATUS 0xC
#define UART_TX_FULL (1 << 21)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while ((*UART_REG(UART_STATUS) & UART_TX_FULL));
    *UART_REG(UART_WFIFO) = ch;
}
#elif defined(BOARD_ultra96v2)
/* Use UART1 available through USB-to-JTAG/UART pod */
#define UART_BASE 0x00ff010000
#define R_UART_CHANNEL_STS          0x2C
#define UART_CHANNEL_STS_TXEMPTY    0x08
#define UART_CHANNEL_STS_TACTIVE    0x800
#define R_UART_TX_RX_FIFO           0x30

static void uart_init(void) {}

static void putc(uint8_t ch)
{
    while (!(*UART_REG(R_UART_CHANNEL_STS) & UART_CHANNEL_STS_TXEMPTY)) {};
    while (*UART_REG(R_UART_CHANNEL_STS) & UART_CHANNEL_STS_TACTIVE) {};

    *((volatile uint32_t *)(UART_BASE + R_UART_TX_RX_FIFO)) = ch;
}
#elif defined(BOARD_qemu_virt_aarch64)
#define UART_BASE                 0x9000000
#define PL011_TCR                 0x030
#define PL011_UARTDR              0x000
#define PL011_UARTFR              0x018
#define PL011_UARTFR_TXFF         (1 << 5)
#define PL011_CR_UART_EN          (1 << 0)
#define PL011_CR_TX_EN            (1 << 8)

static void uart_init()
{
    /* Enable the device and transmit */
    *UART_REG(PL011_TCR) |= (PL011_CR_TX_EN | PL011_CR_UART_EN);
}

static void putc(uint8_t ch)
{
    while ((*UART_REG(PL011_UARTFR) & PL011_UARTFR_TXFF) != 0);
    *UART_REG(PL011_UARTDR) = ch;
}

#elif defined(BOARD_rpi4b_1gb) || defined(BOARD_rpi4b_2gb) || defined(BOARD_rpi4b_4gb) || defined(BOARD_rpi4b_8gb)
#define UART_BASE 0xfe215040
#define MU_IO 0x00
#define MU_LSR 0x14
#define MU_LSR_TXIDLE (1 << 6)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while (!(*UART_REG(MU_LSR) & MU_LSR_TXIDLE));
    *UART_REG(MU_IO) = (ch & 0xff);
}
#elif defined(BOARD_rockpro64)
#define UART_BASE   0xff1a0000
#define UTHR        0x0
#define ULSR        0x14
#define ULSR_THRE   (1 << 5)

static void uart_init() {}

static void putc(uint8_t ch)
{
    while ((*UART_REG(ULSR) & ULSR_THRE) == 0);
    *UART_REG(UTHR) = ch;
}

#elif defined(ARCH_riscv64)
#define SBI_CONSOLE_PUTCHAR 1

// TODO: remove, just do straight ASM
#define SBI_CALL(which, arg0, arg1, arg2) ({            \
    register uintptr_t a0 asm ("a0") = (uintptr_t)(arg0);   \
    register uintptr_t a1 asm ("a1") = (uintptr_t)(arg1);   \
    register uintptr_t a2 asm ("a2") = (uintptr_t)(arg2);   \
    register uintptr_t a7 asm ("a7") = (uintptr_t)(which);  \
    asm volatile ("ecall"                   \
              : "+r" (a0)               \
              : "r" (a1), "r" (a2), "r" (a7)        \
              : "memory");              \
    a0;                         \
})

#define SBI_CALL_1(which, arg0) SBI_CALL(which, arg0, 0, 0)

static void uart_init()
{
    /* Nothing to do, OpenSBI will do UART init for us. */
}

static void putc(uint8_t ch)
{
    SBI_CALL_1(SBI_CONSOLE_PUTCHAR, ch);
}
#else
#error Board not defined
#endif

static void puts(const char *s)
{
#if PRINTING
    while (*s) {
        if (*s == '\n') {
            putc('\r');
        }
        putc(*s);
        s++;
    }
#endif
}

static char hexchar(unsigned int v)
{
    return v < 10 ? '0' + v : ('a' - 10) + v;
}

static void puthex32(uint32_t val)
{
    char buffer[8 + 3];
    buffer[0] = '0';
    buffer[1] = 'x';
    buffer[8 + 3 - 1] = 0;
    for (unsigned i = 8 + 1; i > 1; i--) {
        buffer[i] = hexchar(val & 0xf);
        val >>= 4;
    }
    puts(buffer);
}

static void puthex64(uint64_t val)
{
    char buffer[16 + 3];
    buffer[0] = '0';
    buffer[1] = 'x';
    buffer[16 + 3 - 1] = 0;
    for (unsigned i = 16 + 1; i > 1; i--) {
        buffer[i] = hexchar(val & 0xf);
        val >>= 4;
    }
    puts(buffer);
}

#ifdef ARCH_aarch64
static void puthex(uintptr_t val)
{
#if WORD_SIZE == 32
    puthex32(val);
#else
    puthex64(val);
#endif
}

/* Returns the current exception level */
static enum el current_el(void)
{
    /* See: C5.2.1 CurrentEL */
    uint32_t val;
    asm volatile("mrs %x0, CurrentEL" : "=r"(val) :: "cc");
    /* bottom two bits are res0 */
    return (enum el) val >> 2;
}

static char *el_to_string(enum el el)
{
    switch (el) {
    case EL0:
        return "EL0";
    case EL1:
        return "EL1";
    case EL2:
        return "EL2";
    case EL3:
        return "EL3";
    }

    return "<invalid el>";
}

static char *ex_to_string(uintptr_t ex)
{
    switch (ex) {
    case 0:
        return "Synchronous (Current Exception level with SP_EL0)";
    case 1:
        return "IRQ (Current Exception level with SP_EL0)";
    case 2:
        return "FIQ (Current Exception level with SP_EL0)";
    case 3:
        return "SError (Current Exception level with SP_EL0)";
    case 4:
        return "Synchronous (Current Exception level with SP_ELx)";
    case 5:
        return "IRQ (Current Exception level with SP_ELx)";
    case 6:
        return "FIQ (Current Exception level with SP_ELx)";
    case 7:
        return "SError (Current Exception level with SP_ELx)";
    case 8:
        return "Synchronous 64-bit EL0";
    case 9:
        return "IRQ 64-bit EL0";
    case 10:
        return "FIQ 64-bit EL0";
    case 11:
        return "SError 64-bit EL0";
    case 12:
        return "Synchronous 32-bit EL0";
    case 13:
        return "IRQ 32-bit EL0";
    case 14:
        return "FIQ 32-bit EL0";
    case 15:
        return "SError 32-bit EL0";
    }
    return "<invalid ex>";
}

static char *ec_to_string(uintptr_t ec)
{
    switch (ec) {
    case 0:
        return "Unknown reason";
    case 1:
        return "Trapped WFI or WFE instruction execution";
    case 3:
        return "Trapped MCR or MRC access with (coproc==0b1111) this is not reported using EC 0b000000";
    case 4:
        return "Trapped MCRR or MRRC access with (coproc==0b1111) this is not reported using EC 0b000000";
    case 5:
        return "Trapped MCR or MRC access with (coproc==0b1110)";
    case 6:
        return "Trapped LDC or STC access";
    case 7:
        return "Access to SVC, Advanced SIMD or floating-point functionality trapped";
    case 12:
        return "Trapped MRRC access with (coproc==0b1110)";
    case 13:
        return "Branch Target Exception";
    case 17:
        return "SVC instruction execution in AArch32 state";
    case 21:
        return "SVC instruction execution in AArch64 state";
    case 24:
        return "Trapped MSR, MRS or System instruction exuection in AArch64 state, this is not reported using EC 0xb000000, 0b000001 or 0b000111";
    case 25:
        return "Access to SVE functionality trapped";
    case 28:
        return "Exception from a Pointer Authentication instruction authentication failure";
    case 32:
        return "Instruction Abort from a lower Exception level";
    case 33:
        return "Instruction Abort taken without a change in Exception level";
    case 34:
        return "PC alignment fault exception";
    case 36:
        return "Data Abort from a lower Exception level";
    case 37:
        return "Data Abort taken without a change in Exception level";
    case 38:
        return "SP alignment faultr exception";
    case 40:
        return "Trapped floating-point exception taken from AArch32 state";
    case 44:
        return "Trapped floating-point exception taken from AArch64 state";
    case 47:
        return "SError interrupt";
    case 48:
        return "Breakpoint exception from a lower Exception level";
    case 49:
        return "Breakpoint exception taken without a change in Exception level";
    case 50:
        return "Software Step exception from a lower Exception level";
    case 51:
        return "Software Step exception taken without a change in Exception level";
    case 52:
        return "Watchpoint exception from a lower Exception level";
    case 53:
        return "Watchpoint exception taken without a change in Exception level";
    case 56:
        return "BKPT instruction execution in AArch32 state";
    case 60:
        return "BRK instruction execution in AArch64 state";
    }
    return "<invalid EC>";
}
#endif

/*
 * Print out the loader data structure.
 *
 * This doesn't *do anything*. It helps when
 * debugging to verify that the data structures are
 * being interpreted correctly by the loader.
 */
static void print_flags(void)
{
    if (loader_data->flags & FLAG_SEL4_HYP) {
        puts("             seL4 configured as hypervisor\n");
    }
}

static void print_loader_data(void)
{
    puts("LDR|INFO: Flags:                ");
    puthex64(loader_data->flags);
    puts("\n");
    print_flags();
    puts("LDR|INFO: Kernel:      entry:   ");
    puthex64(loader_data->kernel_entry);
    puts("\n");

    puts("LDR|INFO: Root server: physmem: ");
    puthex64(loader_data->ui_p_reg_start);
    puts(" -- ");
    puthex64(loader_data->ui_p_reg_end);
    puts("\nLDR|INFO:              virtmem: ");
    puthex64(loader_data->ui_p_reg_start - loader_data->pv_offset);
    puts(" -- ");
    puthex64(loader_data->ui_p_reg_end - loader_data->pv_offset);
    puts("\nLDR|INFO:              entry  : ");
    puthex64(loader_data->v_entry);
    puts("\n");

    for (uint32_t i = 0; i < loader_data->num_regions; i++) {
        const struct region *r = &loader_data->regions[i];
        puts("LDR|INFO: region: ");
        puthex32(i);
        puts("   addr: ");
        puthex64(r->load_addr);
        puts("   size: ");
        puthex64(r->size);
        puts("   offset: ");
        puthex64(r->offset);
        puts("   type: ");
        puthex64(r->type);
        puts("\n");
    }
}

static void copy_data(void)
{
    const void *base = &loader_data->regions[loader_data->num_regions];
    for (uint32_t i = 0; i < loader_data->num_regions; i++) {
        const struct region *r = &loader_data->regions[i];
        puts("LDR|INFO: copying region ");
        puthex32(i);
        puts("\n");
        memcpy((void *)(uintptr_t)r->load_addr, base + r->offset, r->size);
    }
}

#ifdef ARCH_aarch64
static int ensure_correct_el(void)
{
    enum el el = current_el();

    puts("LDR|INFO: CurrentEL=");
    puts(el_to_string(el));
    puts("\n");

    if (el == EL0) {
        puts("LDR|ERROR: Unsupported initial exception level\n");
        return 1;
    }

    if (el == EL3) {
        puts("LDR|INFO: Dropping from EL3 to EL2(NS)\n");
        switch_to_el2();
        puts("LDR|INFO: Dropped from EL3 to EL2(NS)\n");
        el = EL2;
    }

    if (loader_data->flags & FLAG_SEL4_HYP) {
        if (el != EL2) {
            puts("LDR|ERROR: seL4 configured as a hypervisor, but not in EL2\n");
            return 1;
        } else {
            puts("LDR|INFO: Resetting CNTVOFF\n");
            asm volatile("msr cntvoff_el2, xzr");
        }
    } else {
        if (el == EL2) {
            /* seL4 relies on the timer to be set to a useful value */
            puts("LDR|INFO: Resetting CNTVOFF\n");
            asm volatile("msr cntvoff_el2, xzr");
            puts("LDR|INFO: Dropping from EL2 to EL1\n");
            switch_to_el1();
            puts("LDR|INFO: CurrentEL=");
            el = current_el();
            puts(el_to_string(el));
            puts("\n");
            if (el == EL1) {
                puts("LDR|INFO: Dropped to EL1 successfully\n");
            } else {
                puts("LDR|ERROR: Failed to switch to EL1\n");
                return 1;
            }
        }
    }

    return 0;
}
#endif

static void start_kernel(void)
{
    ((sel4_entry)(loader_data->kernel_entry))(
        loader_data->ui_p_reg_start,
        loader_data->ui_p_reg_end,
        loader_data->pv_offset,
        loader_data->v_entry,
        0,
        0,
        0,
        0
    );
}

#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2) || defined(BOARD_qemu_virt_aarch64)
static void configure_gicv2(void)
{
    /* The ZCU102 start in EL3, and then we drop to EL1(NS).
     *
     * The GICv2 supports security extensions (as does the CPU).
     *
     * The GIC sets any interrupt as either Group 0 or Group 1.
     * A Group 0 interrupt can only be configured in secure mode,
     * while Group 1 interrupts can be configured from non-secure mode.
     *
     * As seL4 runs in non-secure mode, and we want seL4 to have
     * the ability to configure interrupts, at this point we need
     * to put all interrupts into Group 1.
     *
     * GICD_IGROUPn starts at offset 0x80.
     *
     * 0xF901_0000.
     *
     * Future work: On multicore systems the distributor setup
     * only needs to be called once, while the GICC registers
     * should be set for each CPU.
     */
    puts("LDR|INFO: Setting all interrupts to Group 1\n");
    uint32_t gicd_typer = *((volatile uint32_t *)(GICD_BASE + 0x4));
    uint32_t it_lines_number = gicd_typer & 0x1f;
    puts("LDR|INFO: GICv2 ITLinesNumber: ");
    puthex32(it_lines_number);
    puts("\n");

    for (uint32_t i = 0; i <= it_lines_number; i++) {
        *((volatile uint32_t *)(GICD_BASE + 0x80 + (i * 4))) = 0xFFFFFFFF;
    }

    /* For any interrupts to go through the interrupt priority mask
     * must be set appropriately. Only interrupts with priorities less
     * than this mask will interrupt the CPU.
     *
     * seL4 (effectively) sets interrupts to priority 0x80, so it is
     * important to make sure this is greater than 0x80.
     */
    *((volatile uint32_t *)(GICC_BASE + 0x4)) = 0xf0;
}
#endif

#ifdef ARCH_riscv64

/*
 * This is the encoding for the MODE field of the satp register when
 * implementing 39-bit virtual address spaces (known as Sv39).
 */
#define VM_MODE (0x8llu << 60)

#define RISCV_PGSHIFT 12

static inline void enable_mmu(void)
{
    // The RISC-V privileged spec (20211203), section 4.1.11 says that the
    // SFENCE.VMA instruction may need to be executed before or after writing
    // to satp. I don't understand why we do it before compared to after.
    // Need to understand 4.2.1 of the spec.
    asm volatile("sfence.vma" ::: "memory");
    asm volatile(
        "csrw satp, %0\n"
        :
        : "r"(VM_MODE | (uintptr_t)boot_lvl1_pt >> RISCV_PGSHIFT)
        :
    );
    asm volatile("fence.i" ::: "memory");
}
#endif

void relocation_failed(void)
{
    puts("LDR|ERROR: relocation failed, loader destination would overlap current loader location\n");
    while (1);
}

void relocation_log(uint64_t reloc_addr, uint64_t curr_addr)
{
    /* This function is called from assembly before main so we call uart_init here as well. */
    uart_init();
    puts("LDR|INFO: relocating from ");
    puthex64(curr_addr);
    puts(" to ");
    puthex64(reloc_addr);
    puts("\n");
}

void set_exception_handler()
{
#ifdef ARCH_aarch64
    enum el el = current_el();
    if (el == EL2) {
        asm volatile("msr vbar_el2, %0" :: "r"(arm_vector_table));
    }
    /* Since we call the exception handler before we check we're at
     * a valid EL we shouldn't assume we are at EL1 or higher. */
    if (el != EL0) {
        asm volatile("msr vbar_el1, %0" :: "r"(arm_vector_table));
    }
#elif ARCH_riscv64
    /* Don't do anything on RISC-V since we always are in S-mode so M-mode
     * will catch our faults (e.g SBI). */
#else
#error "Unsupported architecture for set_exception_handler"
#endif
}

int main(void)
{
    uart_init();
    /* After any UART initialisation is complete, setup an arch-specific exception
     * handler in case we fault somewhere in the loader. */
    set_exception_handler();

    puts("LDR|INFO: altloader for seL4 starting\n");
    /* Check that the loader magic number is set correctly */
    if (loader_data->magic != MAGIC) {
        puts("LDR|ERROR: mismatch on loader data structure magic number\n");
        goto fail;
    }

#ifdef ARCH_riscv64
    puts("LDR|INFO: configured with FIRST_HART_ID ");
    puthex32(FIRST_HART_ID);
    puts("\n");
#endif

    print_loader_data();

    /* past here we have trashed u-boot so any errors should go to the
     * fail label; it's not possible to return to U-boot
     */
    copy_data();

#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2) || defined(BOARD_qemu_virt_aarch64)
    configure_gicv2();
#endif

#ifdef ARCH_aarch64
    int r;
    enum el el;
    r = ensure_correct_el();
    if (r != 0) {
        goto fail;
    }

    puts("LDR|INFO: enabling MMU\n");
    el = current_el();
    if (el == EL1) {
        el1_mmu_enable();
    } else if (el == EL2) {
        el2_mmu_enable();
    } else {
        puts("LDR|ERROR: unknown EL level for MMU enable\n");
    }
#elif defined(ARCH_riscv64)
    puts("LDR|INFO: enabling MMU\n");
    enable_mmu();
#endif

    puts("LDR|INFO: jumping to kernel\n");
    start_kernel();

    puts("LDR|ERROR: seL4 Loader: Error - KERNEL RETURNED\n");

fail:
    /* Note: can't usefully return to U-Boot once we are here. */
    /* IMPROVEMENT: use SMC SVC call to try and power-off / reboot system.
     * or at least go to a WFI loop
     */
    for (;;) {
    }
}
#ifdef ARCH_aarch64
void exception_handler(uintptr_t ex)
{
    /* Read ESR/FSR based on the exception level we're at. */
    uint64_t esr;
    uintptr_t far;

    if (loader_data->flags & FLAG_SEL4_HYP) {
        asm volatile("mrs %0, ESR_EL2" : "=r"(esr) :: "cc");
        asm volatile("mrs %0, FAR_EL2" : "=r"(far) :: "cc");
    } else {
        asm volatile("mrs %0, ESR_EL1" : "=r"(esr) :: "cc");
        asm volatile("mrs %0, FAR_EL1" : "=r"(far) :: "cc");
    }

    uintptr_t ec = (esr >> 26) & 0x3f;
    puts("\nLDR|ERROR: loader trapped exception: ");
    puts(ex_to_string(ex));
    if (loader_data->flags & FLAG_SEL4_HYP) {
        puts("\n    esr_el2: ");
    } else {
        puts("\n    esr_el1: ");
    }
    puthex(esr);
    puts("\n    ec: ");
    puthex32(ec);
    puts(" (");
    puts(ec_to_string(ec));
    puts(")\n    il: ");
    puthex((esr >> 25) & 1);
    puts("\n    iss: ");
    puthex(esr & MASK(24));
    puts("\n    far: ");
    puthex(far);
    puts("\n");

    for (unsigned i = 0; i < 32; i++)  {
        puts("    reg: ");
        puthex32(i);
        puts(": ");
        puthex(exception_register_state[i]);
        puts("\n");
    }

    for (;;) {
    }
}
#endif
