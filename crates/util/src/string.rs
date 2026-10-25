#[macro_export]
macro_rules! write_str {
    ($dst:expr, $($arg:tt)*) => {{
        use std::fmt::Write as _;
        write!($dst, $($arg)*).expect("writing to a String cannot fail")
    }};
}
