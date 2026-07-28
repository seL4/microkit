/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <stddef.h>
#include <microkit.h>

#include "elf.h"

extern const unsigned char __pc_start[];
extern const unsigned char __pc_end[];

#define LOADEE_MR (0xC00000)
#define LOADEE_ENTRY (0x280000)

static uint8_t restart_count = 0;

static char hexchar(unsigned int v)
{
    return v < 10 ? '0' + v : 'a' + (v - 10);
}

static void puthex64(uint64_t x)
{
    char tmp[17];
    unsigned i = 16;

    tmp[16] = 0;

    do {
        tmp[--i] = hexchar(x & 0xf);
        x >>= 4;
    } while (x);

    microkit_dbg_puts(&tmp[i]);
}

void *custom_memcpy(void *restrict dest, const void *restrict src, size_t n)
{
    unsigned char *d = dest;
    const unsigned char *s = src;

#ifdef __GNUC__

#if __BYTE_ORDER == __LITTLE_ENDIAN
#define LS >>
#define RS <<
#else
#define LS <<
#define RS >>
#endif

    typedef uint32_t __attribute__((__may_alias__)) u32;
    uint32_t w, x;

    for (; (uintptr_t)s % 4 && n; n--) {
        *d++ = *s++;
    }

    if ((uintptr_t)d % 4 == 0) {
        for (; n >= 16; s += 16, d += 16, n -= 16) {
            *(u32 *)(d + 0) = *(u32 *)(s + 0);
            *(u32 *)(d + 4) = *(u32 *)(s + 4);
            *(u32 *)(d + 8) = *(u32 *)(s + 8);
            *(u32 *)(d + 12) = *(u32 *)(s + 12);
        }
        if (n & 8) {
            *(u32 *)(d + 0) = *(u32 *)(s + 0);
            *(u32 *)(d + 4) = *(u32 *)(s + 4);
            d += 8;
            s += 8;
        }
        if (n & 4) {
            *(u32 *)(d + 0) = *(u32 *)(s + 0);
            d += 4;
            s += 4;
        }
        if (n & 2) {
            *d++ = *s++;
            *d++ = *s++;
        }
        if (n & 1) {
            *d = *s;
        }
        return dest;
    }

    if (n >= 32) switch ((uintptr_t)d % 4) {
        case 1:
            w = *(u32 *)s;
            *d++ = *s++;
            *d++ = *s++;
            *d++ = *s++;
            n -= 3;
            for (; n >= 17; s += 16, d += 16, n -= 16) {
                x = *(u32 *)(s + 1);
                *(u32 *)(d + 0) = (w LS 24) | (x RS 8);
                w = *(u32 *)(s + 5);
                *(u32 *)(d + 4) = (x LS 24) | (w RS 8);
                x = *(u32 *)(s + 9);
                *(u32 *)(d + 8) = (w LS 24) | (x RS 8);
                w = *(u32 *)(s + 13);
                *(u32 *)(d + 12) = (x LS 24) | (w RS 8);
            }
            break;
        case 2:
            w = *(u32 *)s;
            *d++ = *s++;
            *d++ = *s++;
            n -= 2;
            for (; n >= 18; s += 16, d += 16, n -= 16) {
                x = *(u32 *)(s + 2);
                *(u32 *)(d + 0) = (w LS 16) | (x RS 16);
                w = *(u32 *)(s + 6);
                *(u32 *)(d + 4) = (x LS 16) | (w RS 16);
                x = *(u32 *)(s + 10);
                *(u32 *)(d + 8) = (w LS 16) | (x RS 16);
                w = *(u32 *)(s + 14);
                *(u32 *)(d + 12) = (x LS 16) | (w RS 16);
            }
            break;
        case 3:
            w = *(u32 *)s;
            *d++ = *s++;
            n -= 1;
            for (; n >= 19; s += 16, d += 16, n -= 16) {
                x = *(u32 *)(s + 3);
                *(u32 *)(d + 0) = (w LS 8) | (x RS 24);
                w = *(u32 *)(s + 7);
                *(u32 *)(d + 4) = (x LS 8) | (w RS 24);
                x = *(u32 *)(s + 11);
                *(u32 *)(d + 8) = (w LS 8) | (x RS 24);
                w = *(u32 *)(s + 15);
                *(u32 *)(d + 12) = (x LS 8) | (w RS 24);
            }
            break;
        }
    if (n & 16) {
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
    }
    if (n & 8) {
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
    }
    if (n & 4) {
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
        *d++ = *s++;
    }
    if (n & 2) {
        *d++ = *s++;
        *d++ = *s++;
    }
    if (n & 1) {
        *d = *s;
    }
    return dest;
#endif

    for (; n; n--) {
        *d++ = *s++;
    }
    return dest;
}

void *custom_memset(void *dest, int c, size_t n)
{
    unsigned char *s = dest;
    size_t k;

    /* Fill head and tail with minimal branching. Each
     * conditional ensures that all the subsequently used
     * offsets are well-defined and in the dest region. */

    if (!n) {
        return dest;
    }
    s[0] = c;
    s[n - 1] = c;
    if (n <= 2) {
        return dest;
    }
    s[1] = c;
    s[2] = c;
    s[n - 2] = c;
    s[n - 3] = c;
    if (n <= 6) {
        return dest;
    }
    s[3] = c;
    s[n - 4] = c;
    if (n <= 8) {
        return dest;
    }

    /* Advance pointer to align it at a 4-byte boundary,
     * and truncate n to a multiple of 4. The previous code
     * already took care of any head/tail that get cut off
     * by the alignment. */

    k = -(uintptr_t)s & 3;
    s += k;
    n -= k;
    n &= -4;

#ifdef __GNUC__
    typedef uint32_t __attribute__((__may_alias__)) u32;
    typedef uint64_t __attribute__((__may_alias__)) u64;

    u32 c32 = ((u32) - 1) / 255 * (unsigned char)c;

    /* In preparation to copy 32 bytes at a time, aligned on
     * an 8-byte bounary, fill head/tail up to 28 bytes each.
     * As in the initial byte-based head/tail fill, each
     * conditional below ensures that the subsequent offsets
     * are valid (e.g. !(n<=24) implies n>=28). */

    *(u32 *)(s + 0) = c32;
    *(u32 *)(s + n - 4) = c32;
    if (n <= 8) {
        return dest;
    }
    *(u32 *)(s + 4) = c32;
    *(u32 *)(s + 8) = c32;
    *(u32 *)(s + n - 12) = c32;
    *(u32 *)(s + n - 8) = c32;
    if (n <= 24) {
        return dest;
    }
    *(u32 *)(s + 12) = c32;
    *(u32 *)(s + 16) = c32;
    *(u32 *)(s + 20) = c32;
    *(u32 *)(s + 24) = c32;
    *(u32 *)(s + n - 28) = c32;
    *(u32 *)(s + n - 24) = c32;
    *(u32 *)(s + n - 20) = c32;
    *(u32 *)(s + n - 16) = c32;

    /* Align to a multiple of 8 so we can fill 64 bits at a time,
     * and avoid writing the same bytes twice as much as is
     * practical without introducing additional branching. */

    k = 24 + ((uintptr_t)s & 4);
    s += k;
    n -= k;

    /* If this loop is reached, 28 tail bytes have already been
     * filled, so any remainder when n drops below 32 can be
     * safely ignored. */

    u64 c64 = c32 | ((u64)c32 << 32);
    for (; n >= 32; n -= 32, s += 32) {
        *(u64 *)(s + 0) = c64;
        *(u64 *)(s + 8) = c64;
        *(u64 *)(s + 16) = c64;
        *(u64 *)(s + 24) = c64;
    }
#else
    /* Pure C fallback with no aliasing violations. */
    for (; n; n--, s++) {
        *s = c;
    }
#endif

    return dest;
}

void custom_elfload(void *dest_vaddr, const Elf64_Ehdr *ehdr)
{
    Elf64_Phdr *phdr = (Elf64_Phdr *)((char *)ehdr + ehdr->e_phoff);

    for (int i = 0; i < ehdr->e_phnum; i++) {
        if (phdr[i].p_type != PT_LOAD) {
            continue;
        }
        void *src = (char *)ehdr + phdr[i].p_offset;
        void *dest = (void *)(dest_vaddr + phdr[i].p_vaddr - ehdr->e_entry);

        custom_memcpy(
            dest,
            src,
            phdr[i].p_filesz
        );

        if (phdr[i].p_memsz > phdr[i].p_filesz) {
            seL4_Word bss_size =
                phdr[i].p_memsz - phdr[i].p_filesz;
            custom_memset(
                (char *)dest + phdr[i].p_filesz,
                0,
                bss_size
            );
        }
    }
}

int custom_memcmp(const unsigned char *s1, const unsigned char *s2, int n)
{
    for (int i = 0; i < n; i++) {
        if (s1[i] != s2[i]) {
            return (s1[i] - s2[i]);
        }
    }
    return 0;
}

void init(void)
{
    microkit_dbg_puts(">> loader: hi\n");
}

void notified(microkit_channel ch)
{
}

seL4_MessageInfo_t protected(microkit_channel ch, microkit_msginfo msginfo)
{
    return microkit_msginfo_new(0, 0);
}

seL4_Bool fault(microkit_child child, microkit_msginfo msginfo, microkit_msginfo *reply_msginfo)
{
    seL4_Word label = microkit_msginfo_get_label(msginfo);
    if (label == seL4_Fault_VMFault) {
        seL4_Word ip = microkit_mr_get(seL4_VMFault_IP);
        seL4_Word address = microkit_mr_get(seL4_VMFault_Addr);
        seL4_Word notidy_flag = ip | address;
        if (notidy_flag) {
            microkit_dbg_puts(">> seL4_Fault_VMFault\n");
            microkit_dbg_puts(">> Fault address: '");
            puthex64(address);
            microkit_dbg_puts("'\n");
            microkit_dbg_puts(">> Fault instruction pointer: '");
            puthex64(ip);
            microkit_dbg_puts("'\n");
        } else {
            microkit_dbg_puts(">> receive the first fault from an empty pd with id; '");
            puthex64(child);
            microkit_dbg_puts("'\n");
        }
    }

    const Elf64_Ehdr *hdr = (const Elf64_Ehdr *)(__pc_start);
    if (restart_count == 0) {
        if (custom_memcmp(hdr->e_ident, (const unsigned char *)ELFMAG, SELFMAG) != 0) {
            while (1);
        }
        custom_elfload((char *)(LOADEE_MR), hdr);
    }
    if (restart_count < 3) {
        microkit_pd_restart(child, hdr->e_entry);
    } else {
        microkit_pd_stop(child);
        microkit_dbg_puts(">> loader: too many restarts - PD stopped\n");
    }
    restart_count++;

    /* We explicitly restart the thread so we do not need to 'reply' to the fault. */
    return seL4_False;
}