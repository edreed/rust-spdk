#![allow(dead_code)]
use std::{
    mem::{transmute, MaybeUninit},
    os::raw::c_void,
    pin::Pin,
    ptr::null_mut,
};

use spdk_sys::{
    spdk_sock, spdk_sock_group, spdk_sock_group_add_sock, spdk_sock_group_close,
    spdk_sock_group_create, spdk_sock_group_get_ctx, spdk_sock_group_poll,
    spdk_sock_group_remove_sock,
};

use crate::{
    errors::Errno,
    task::{Polled, Poller},
    to_result,
};

use super::AsRawSock;

pub(crate) trait SocketEvent: AsRawSock {
    fn handle_event(&mut self);
}

pub(crate) trait SocketGroupEvent {
    fn handle_event<S>(&mut self, sock: &mut S)
    where
        S: SocketEvent;
}

struct SocketGroupInner<C>
where
    C: 'static,
{
    group: *mut spdk_sock_group,
    ctx: C,
}

impl<C> SocketGroupInner<C>
where
    C: Default + Unpin + 'static,
{
    fn new_in_place(this: Pin<&mut MaybeUninit<Self>>) {
        let this: &mut SocketGroupInner<C> = this.get_mut().write(Self {
            group: null_mut(),
            ctx: Default::default(),
        });

        this.group = unsafe { spdk_sock_group_create(this as *mut _ as *mut _) };

        assert!(!this.group.is_null(), "failed to allocate socket group");
    }
}

impl<C> SocketGroupInner<C>
where
    C: 'static,
{
    fn with_context_in_place<F>(this: Pin<&mut MaybeUninit<Self>>, init_fn: F)
    where
        F: FnOnce(Pin<&mut MaybeUninit<C>>),
    {
        let this: &mut MaybeUninit<SocketGroupInner<MaybeUninit<C>>> =
            unsafe { transmute(Pin::get_unchecked_mut(this)) };
        let this = this.write(SocketGroupInner::<MaybeUninit<C>> {
            group: null_mut(),
            ctx: MaybeUninit::zeroed(),
        });

        init_fn(unsafe { Pin::new_unchecked(&mut this.ctx) });

        let this: &mut Self = unsafe { transmute(this) };

        this.group = unsafe { spdk_sock_group_create(this as *mut _ as *mut _) };

        assert!(!this.group.is_null(), "failed to allocate socket group");
    }
}

impl<C> Drop for SocketGroupInner<C>
where
    C: 'static,
{
    fn drop(&mut self) {
        unsafe {
            to_result! { spdk_sock_group_close(&mut self.group as *mut _) }
                .expect("socket group closed");
        }
    }
}

impl<C> Polled for SocketGroupInner<C> {
    fn poll(self: Pin<&mut Self>) -> bool {
        unsafe { spdk_sock_group_poll(self.group) != 0 }
    }
}

pub(crate) struct SocketGroup<C: 'static = ()>(Poller<SocketGroupInner<C>>);

impl<C> SocketGroup<C>
where
    C: SocketGroupEvent + Unpin + Default + 'static,
{
    pub(crate) fn new() -> Self {
        Self(Poller::new_in_place(SocketGroupInner::new_in_place))
    }
}

impl<C> SocketGroup<C>
where
    C: SocketGroupEvent + Unpin + 'static,
{
    pub(crate) fn with_context(ctx: C) -> Self {
        Self(Poller::new_in_place(|this| {
            SocketGroupInner::with_context_in_place(this, |uninit_ctx| {
                uninit_ctx.get_mut().write(ctx);
            })
        }))
    }
}

impl<C> SocketGroup<C>
where
    C: SocketGroupEvent + 'static,
{
    unsafe extern "C" fn handle_event<S>(
        arg: *mut c_void,
        group: *mut spdk_sock_group,
        _sock: *mut spdk_sock,
    ) where
        S: SocketEvent,
    {
        let group = &mut *(unsafe { spdk_sock_group_get_ctx(group) } as *mut Self);
        let sock = &mut *(arg as *mut S);

        group.0.polled_mut().ctx.handle_event(sock);
    }

    pub(crate) fn add<S>(&mut self, sock: &S) -> Result<(), Errno>
    where
        S: SocketEvent,
    {
        to_result! {
            unsafe {
                spdk_sock_group_add_sock(
                    self.0.polled().group,
                    sock.as_raw_sock(),
                    Some(Self::handle_event::<S>),
                    &sock as *const _ as *mut _)
            }
        }
    }

    pub(crate) fn remove<S>(&mut self, sock: &S) -> Result<(), Errno>
    where
        S: SocketEvent,
    {
        to_result!(unsafe {
            spdk_sock_group_remove_sock(self.0.polled().group, sock.as_raw_sock())
        })
    }
}

impl SocketGroupEvent for () {
    fn handle_event<S>(&mut self, sock: &mut S)
    where
        S: SocketEvent,
    {
        sock.handle_event();
    }
}
