use std::{marker::PhantomData, ptr::{null_mut, NonNull}};

use spdk_sys::{spdk_sock, spdk_sock_group, spdk_sock_group_add_sock, spdk_sock_group_create, spdk_sock_group_remove_sock};

use crate::{
    errors::Errno,

    to_result,
};

use super::AsRawSock;

pub(crate) trait EventHandler : AsRawSock {
    fn handle_event(&self);
}

pub(crate) struct SocketGroupMembership<'a, T> {
    group: NonNull<spdk_sock_group>,
    sock: NonNull<spdk_sock>,
    data: PhantomData<&'a T>
}

impl<'a, T> Drop for SocketGroupMembership<'a, T> {
    fn drop(&mut self) {
        to_result!(unsafe {
            spdk_sock_group_remove_sock(self.group.as_ptr(), self.sock.as_ptr())
        }).expect("spdk_sock removed");
    }
}

pub(crate) struct SocketGroup(NonNull<spdk_sock_group>);

impl SocketGroup {
    pub(crate) fn new() -> Self {
        let group = NonNull::new(unsafe { spdk_sock_group_create(null_mut()) })
            .expect("spdk_sock_group allocated");

        Self(group)
    }

    pub(crate) fn add<'a, T: EventHandler>(&mut self, sock: &'a T) -> Result<SocketGroupMembership<'a, T>, Errno> {
        let raw_sock = sock.as_raw_sock();

        to_result!(unsafe {
            spdk_sock_group_add_sock(
                self.0.as_ptr(),
                raw_sock,
                cb_fn,
                cb_arg
            )})
            .map(|_| {
                SocketGroupMembership {
                    group: self.0,
                    sock: unsafe { NonNull::new_unchecked(raw_sock) },
                    data: PhantomData,
                }
            })
    }

    pub(crate) fn remove<T: AsRawSock>(&mut self, sock: &T) -> Result<(), Errno> {
        to_result!(unsafe {
            spdk_sock_group_remove_sock(self.0.as_ptr(), sock.as_raw_sock())
        })
    }
}