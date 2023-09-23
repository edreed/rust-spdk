use std::{
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

use futures::Future;

/// Yield execution back to the SPDK Event Framework.
pub async fn yield_now() {
    struct YieldNow {
        yielded: bool,
    }

    impl Future for YieldNow {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.yielded {
                return Poll::Ready(());
            }

            self.yielded = true;

            ctx.waker().wake_by_ref();

            Poll::Pending
        }
    }

    YieldNow{ yielded: false }.await
}
