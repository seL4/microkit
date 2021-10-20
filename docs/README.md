


## Building the example

    $ cd example
    $ make

The output will be `hello.sysbin` which is, eventually, intended to by a file that can be directly loaded onto a board using the board's regular bootloader.


## Bootstrapping memory

In a perfect world, the newly propose [boot interface](https://sel4.discourse.group/t/pre-rfc-boot-interface/295/8) would already be in place.
This would allow for all the memory regions to be directly initialized by the loader.
However, pushing those changes will require significant time (and possibly reverification).
As such an alternative approach is required.
The initial images will need to be packed into the initial tasks image, and then the initial task (i.e. the platform runtime) will need to move memory about appropriately.

The root server ELF image will need to be manipulated appropriately to include a packed memory image (as well as other data structures).

## seL4 Core Platform Library

The header and source files for the library are in the `libsel4cp` directory.

To compile against the library you need flags such as:

    -I$(LIBSEL4CP)/include -L$(LIBSEL4CP) -llibsel4cp

Where LIBSELCP variable refers to the `libsel4cp` directory.

Note: current state is that this is used in place.
Ftuture state is that this shall be merged with an seL4 SDK to simplify distribution and configuration.

### Functions

This section provides a brief summary of the functions made available in the seL4 Core Platform Library.

* `void sel4cp_dbg_putc(int c)`: Output a single character to the debug console.
* `void sel4cp_dbg_puts(const char *s)`: Output a string to the debug console.

