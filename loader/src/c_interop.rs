#[cfg(not(test))]
mod real_hardware {
    use core::ffi::c_char;
    use core::ffi::CStr;
    use core::fmt;
    use core::fmt::Write;
    use core::panic::PanicInfo;

    unsafe extern "C" {
        safe fn fail() -> !;
        // safe fn putc(c: c_char);
        unsafe fn puts(s: *const c_char);
    }

    /// Exposed only for print macro.
    #[doc(hidden)]
    pub(crate) struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> Result<(), fmt::Error> {
            for c in s.bytes() {
                unsafe { puts(CStr::from_bytes_with_nul_unchecked(&[c, 0]).as_ptr()) };
            }
            Ok(())
        }
    }

    #[macro_export]
    macro_rules! __print {
        ($($arg:tt)*) => {{
            use core::fmt::Write;
            $crate::c_interop::Writer{}.write_fmt(format_args!($($arg)*)).unwrap()
        }}
    }

    #[macro_export]
    macro_rules! __println {
        () => {{
            $crate::__print!("\n");
        }};

        ($($arg:tt)*) => {{
            $crate::__print!("{}\n", format_args!($($arg)*));
        }}
    }

    pub(crate) use __println as println;

    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        println!("panicked");

        if writeln!(Writer, "{}", info).is_err() {
            // If writeln!() fails (which it should never as our fmt::Write) never
            // fails, then just don't print the extra information.
            println!("panicked (information unknown)");
        }

        fail();
    }
}

#[cfg(test)]
mod for_tests {
    extern crate std;
    pub(crate) use std::println;
}

#[cfg(test)]
pub(crate) use for_tests::*;

#[cfg(not(test))]
pub(crate) use real_hardware::*;
