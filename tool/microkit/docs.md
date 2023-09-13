# Description

## Emulating the kernel boot

1. Building the system requires knowledge of the untyped objects
that the kernel has. To achieve this we must emulate what the
kernel does during boot.the kernel boot
The inputs for emulating a kernel boot are the kernel ELF image,
the region of physical memory used by the initial task, and the
reserved region of physical memory.

### Determine physical memory region for initial task

1. We use the ELF file to tell us, we should allocate it.
2. It must be a single region.


## Things that need improvement

Right now the memory for the initial task region is determine by the ELF file.
This is dumb. It should be allocated.
We need a physical memory allocator for the platform.
The physical memory allocator should take into account:

1/ Kernel physical memory.
2/ Loader physical memory.
3/ Boot loader physical memory.



    #
    # Building the system has a problem:
    #
    #   In order to accurately build the system it is necessary to know, in advance,
    #
    #   1/ the amount of memory to use for the 'invocation table' which is used by the
    #   monitor to setup the system.
    #
    #   2/ the size of the system CNode used to hold caps for the initial objects.
    #
    #   However, it is also necessary to *build* the system actually know how many
    #   invocations are required to setup the system, or the number of caps required.
    #
    #   The size of the invocation table or initial CNode can potentially change the
    #   number of invocations or caps required for the system!
    #
    #   The approach taken to solve this is:
    #
    #     1/ At the beginning we start with the minimum possible sizes for the invocation
    #     table and system CNode.
    #
    #     2/ We build the system based on this assumption
    #
    #     3/ We check the number of invocations and caps required for the build system.
    #     If it fits in the current invocation table and system CNode, we reach a stopping
    #     condition.
    #     Otherwise we increase both the invocation table size and system cnode size
    #     to fit the required number of invocations and caps, we then repeat from step #2
    #
    #   In almost all cases we'd expect to reach a stable point after two iterations
    #   of this process, however it is possible that increasing the size of the invocation
    #   table / system CNode creates a system that requires more invocations of caps
    #   resulting in another iteration.
    #
    #   Note: invocation table size must be a multiple of page size, while system cnode
    #   size must be a power of two. This means they will, in most circumstances, be
    #   overallocated on the first iteration, so even a modest increase in invocations
    #   or caps will not required a subsequent increase.
    #
    #   Note: the algorithm never decreases the size of the invocation table or
    #   system CNode. It is possible (I think!) although very unlikely that a larger
    #   invocation table or system CNode may result in fewer invocations or caps being
    #   required. If the sizes were decreased after an iteration it would be possible
    #   (although exceedingly unlikely) for the algorithm to never terminate as it bounces
    #   between to constantly increasing then decreasing sizes.
    #
