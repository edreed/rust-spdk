/// Convert an SPDK integer status return value to a [`Result`].
/// 
/// A status return value either indicates success (0) or an error (<0). A
/// success return is converted to `Ok(())` while an error return is converted
/// to `Err(Errno)` containing the error code.
#[macro_export]
macro_rules! to_result {
    ($r:expr) => {
        match crate::errors::Errno($r) {
            crate::errors::Errno(0) => Ok(()),
            crate::errors::Errno(e) if e < 0 => Err(crate::errors::Errno(-e)),
            _ => unreachable!()
        }
    };
}

/// Convert an SPDK integer size return value to a [`Result`].
/// 
/// Some SPDK functions return a size on success. This is indicated by a return
/// value that is `>= 0`. A success return is converted to `Ok(size)` where
/// `size` is the positive integer value returned. An error return (`< 0`) is
/// converted to `Err(Errno)` containing the error code.
#[macro_export]
macro_rules! to_result_size {
    ($r:expr) => {
        match crate::errors::Errno($r) {
            crate::errors::Errno(e) if e < 0 => Err(crate::errors::Errno(-e)),
            crate::errors::Errno(s) => Ok(s as usize)
        }
    };
}

/// Convert an SPDK integer return value to a [`Poll`] value indicating whether
/// an asynchronous operation is pending or a result is available.
/// 
/// A return value of `0` indicates that the asynchronous operation successfully
/// started and this macro will return `Poll::Pending`. A return value of `< 0`
/// indicates an error and this macro will return `Poll::Ready(Err(Errno))`
/// 
/// [`Poll`]: enum@std::task::Poll
#[macro_export]
macro_rules! to_poll_pending_on_ok {
    ($r:expr) => {
        match crate::to_result!($r) {
            Ok(()) => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Err(e))
        }
    };
}

/// Convert an SPDK integer return value to a [`Poll`] value indicating whether
/// an asynchronous operation is pending or a result is available.
/// 
/// Some SPDK functions indicate synchronous completion by returning a value of
/// `0` and asynchronous operation pending indicated by a specific error code.
/// This macro takes the error code that indicates pending operation as its
/// first argument and the return value of the SPDK function as its second.
/// 
/// A return value of `0` indicates the operation was completed synchronously
/// and this macro will return `Poll::Ready(Ok(()))`. A return value indicating
/// that the asynchronous operation is pending will return `Poll::Pending`.
/// Other return values `< 0` are converted to `Poll::Ready(Err(Errno))`.
/// 
/// [`Poll`]: enum@std::task::Poll
#[macro_export]
macro_rules! to_poll_pending_on_err {
    ($e:expr, $r:expr) => {
        match crate::to_result!($r) {
            Ok(()) => std::task::Poll::Ready(Ok(())),
            Err(e) if e == $e => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Err(e))
        }
    };
}
