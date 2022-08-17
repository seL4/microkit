.extern main

.section ".text.start"

.global _start;
.type _start, %function;
_start:
    la s1, (_stack + 0xff0)
    mv sp, s1
    j main
