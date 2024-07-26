use crate::serial::STD_OUT;

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::serial::print::_print(format_args!($($arg)*)).await);
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub async fn _print(args: core::fmt::Arguments<'_>) {
    let mut std_out = STD_OUT.lock().await;

    core::fmt::write(&mut *std_out, args).expect("STDOUT buffer overflow");
}
