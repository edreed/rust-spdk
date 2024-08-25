#[macro_export]
macro_rules! to_write_result {
    ($r:expr) => {
        if $r == 0 {
            Ok(())
        } else {
            Err(crate::json::serde::Error::WriteFailed)
        }
    };
}

#[macro_export]
macro_rules! to_unexpected_value_result {
    ($r:expr) => {
        if $r >= 0 {
            Ok(())
        } else {
            Err(crate::json::serde::Error::UnexpectedValue)
        }
    };
}
