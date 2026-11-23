#![allow(unused)]
use std::{
    collections::{HashMap, VecDeque},
    ffi::{CStr, CString},
    io::{IoSlice, IoSliceMut},
    marker::PhantomData,
    mem::transmute,
    ops::ControlFlow,
    rc::Rc,
    sync::{Arc, OnceLock},
};

use event_listener::{Event, listener};
use spdk::{
    bdev::{
        Any, BDev, BDevImpl, BDevOps, ClaimType::ReadManyWriteOne, Descriptor, EventHandler,
        EventType, IoChannel, IoChannelImpl, IoError, IoRequest, IoResult, IoType, ModuleInstance,
        ModuleOps, Owned, OwnedBy,
    },
    errors::{EINVAL, ENOTSUP, Errno},
    runtime::{self, wait_for_shutdown},
    sync::{Condvar, SpinLock},
    thread::{
        self, App, AsRawThread, Thread, ThreadCell, Unowned, spawn_local_detached, with_current,
    },
};

enum QueuedIo<'io> {
    Read {
        bufs: &'io mut [IoSliceMut<'io>],
        offset_blocks: u64,
        num_blocks: u64,
    },
    Write {
        bufs: &'io [IoSlice<'io>],
        offset_blocks: u64,
        num_blocks: u64,
    },
    Shutdown,
}

#[derive(Default)]
struct QueuedIoResult<'io> {
    result: SpinLock<Option<IoResult<()>>>,
    condvar: Condvar,
    _marker: PhantomData<QueuedIo<'io>>,
}

impl QueuedIoResult<'_> {
    async fn get_result(&self) -> IoResult<()> {
        self.condvar
            .wait_on_mutex(&self.result, |result| {
                if let Some(res) = result.take() {
                    return ControlFlow::Break(res);
                }

                ControlFlow::Continue(())
            })
            .await
    }

    fn set_result(&self, res: IoResult<()>) {
        *self.result.lock() = Some(res);
        self.condvar.notify_one();
    }
}

struct QueuedIoElem<'io> {
    io: QueuedIo<'io>,
    result: Arc<QueuedIoResult<'io>>,
}

impl<'io> QueuedIoElem<'io> {
    fn new(io: QueuedIo<'io>) -> Self {
        Self {
            io,
            result: Default::default(),
        }
    }
}

#[derive(Default)]
struct Raid5RsWorkerInner<'io> {
    io_queue: VecDeque<QueuedIoElem<'io>>,
    io_available: Event,
}

impl<'io> Raid5RsWorkerInner<'io> {
    fn new() -> Self {
        Default::default()
    }

    fn enqueue_io(&mut self, io: QueuedIo<'io>) -> Arc<QueuedIoResult<'io>> {
        let queued_io_elem = QueuedIoElem::new(io);
        let queued_io_result = queued_io_elem.result.clone();

        self.io_queue.push_back(queued_io_elem);

        self.io_available.notify(1);

        queued_io_result
    }

    fn shutdown(&mut self) -> Arc<QueuedIoResult<'io>> {
        self.enqueue_io(QueuedIo::Shutdown)
    }

    async fn process_io<'child>(&mut self, worker: Arc<Raid5RsWorker<'child>>)
    where
        'child: 'io,
    {
        // FIXME: How do we avoid this cast here to convince the compiler that the descriptor
        // reference lives long enough?
        let child_desc = unsafe {
            transmute::<&Descriptor<'_, Raid5Rs<'_>>, &'io Descriptor<'_, Raid5Rs<'_>>>(
                &worker.child_desc,
            )
        };
        let child_ch = worker.child_desc.io_channel().expect("channel allocated");

        'processing: loop {
            while let Some(elem) = self.io_queue.pop_front() {
                let res = match elem.io {
                    QueuedIo::Read {
                        bufs,
                        offset_blocks,
                        num_blocks,
                    } => {
                        // FIXME: Why does the compiler believe that the lifetime of `child_ch`
                        // won't outlive the async call to `read_vectored_blocks_at` but works just
                        // fine for `write_vectored_blocks_at`? I need to figure this out. For now,
                        // cast its lfimetime to `'io'` as a workaround.
                        unsafe { transmute::<&IoChannel, &'io IoChannel>(&child_ch) }
                            .read_vectored_blocks_at(child_desc, bufs, offset_blocks, num_blocks)
                            .await
                    }
                    QueuedIo::Write {
                        bufs,
                        offset_blocks,
                        num_blocks,
                    } => {
                        child_ch
                            .write_vectored_blocks_at(child_desc, bufs, offset_blocks, num_blocks)
                            .await
                    }
                    QueuedIo::Shutdown => {
                        break 'processing;
                    }
                    _ => {
                        unreachable!()
                    }
                };

                elem.result.set_result(res);
            }

            listener!(&self.io_available => io_available);

            io_available.await;
        }
    }
}

type Raid5RsWorkerThread = Thread<thread::OwnedBy<'static, Raid5RsModule>>;

struct Raid5RsWorker<'child> {
    child_desc: Descriptor<'child, Raid5Rs<'child>>,
    inner: ThreadCell<thread::OwnedBy<'static, Raid5RsModule>, Raid5RsWorkerInner<'child>>,
}

unsafe impl Send for Raid5RsWorker<'_> {}

impl<'child> Raid5RsWorker<'child> {
    async fn new(
        thread: Raid5RsWorkerThread,
        child_desc: Descriptor<'child, Raid5Rs<'child>>,
    ) -> Arc<Self> {
        let this = Arc::new(Self {
            child_desc,
            inner: ThreadCell::new(thread, Raid5RsWorkerInner::new()),
        });

        let this_clone = this.clone();

        this.inner.with(move || {
            move |inner| unsafe {
                spawn_local_detached(async move { inner.process_io(this_clone).await });
            }
        });

        this
    }

    async fn process_io<'io>(&self, io: &'io mut IoRequest<Raid5RsIo>) -> IoResult<()>
    where
        'io: 'child,
    {
        let offset_blocks = io.offset_blocks();
        let num_blocks = io.num_blocks();

        let queued_io = match io.io_type() {
            IoType::Read => QueuedIo::Read {
                bufs: io.buffers_mut(),
                offset_blocks,
                num_blocks,
            },
            IoType::Write => QueuedIo::Write {
                bufs: io.buffers(),
                offset_blocks,
                num_blocks,
            },
            _ => {
                unreachable!()
            }
        };

        let result = self
            .inner
            .with(move || move |inner| inner.enqueue_io(queued_io))
            .await;

        result.get_result().await
    }

    async fn shutdown(&self) {
        let shutdown = self.inner.with(|| |inner| inner.shutdown()).await;

        shutdown.get_result().await.expect("worker shutdown");
    }
}

impl EventHandler for Raid5RsWorker<'_> {
    fn handle_event(&self, event: EventType, device: &BDev<OwnedBy<'_, Self>>) {
        todo!()
    }
}

#[derive(Default)]
struct Raid5RsIo {
    pending: u8,
    succceeded: u8,
    failed: u8,
}

#[derive(Default)]
struct Raid5Rs<'child> {
    workers: Vec<Arc<Raid5RsWorker<'child>>>,
}

impl<'child> Raid5Rs<'child> {
    async fn create_bdev(
        name: &CStr,
        stripe_size: u32,
        children: Vec<&str>,
    ) -> spdk::Result<BDev<Owned>> {
        debug_assert!(
            thread::is_application(),
            "Raid5Rs must be created on the application thread"
        );

        let mut block_size = 0;
        let mut num_blocks = 0;

        // We need at least 3 children
        if children.len() < 3 {
            return Err(EINVAL);
        }

        // Open the children and derive the RAID-5 block size and count.
        let mut child_bdevs = Vec::new();

        for child in children {
            let child = CString::new(child).or(Err(EINVAL))?;
            let child_bdev = BDev::from_name(child.as_c_str())?;

            if block_size == 0 {
                debug_assert!(num_blocks == 0);
                block_size = child_bdev.logical_block_size();
                num_blocks = child_bdev.logical_block_count();
                continue;
            }

            if block_size != child_bdev.logical_block_size()
                || num_blocks != child_bdev.logical_block_count()
            {
                return Err(EINVAL);
            }

            child_bdevs.push(child_bdev);
        }

        // The stripe size must be a multiple of the block size.
        if !stripe_size.is_multiple_of(block_size) {
            return Err(EINVAL);
        }

        // The stripe size muust be equal to or larger than the block size.
        let stripe_blocks = stripe_size / block_size;

        if stripe_blocks == 0 {
            return Err(EINVAL);
        }

        num_blocks *= child_bdevs.len() as u64;

        Raid5RsModule::new_bdev_builder(name, block_size, num_blocks)
            .with_optimal_io_boundary(stripe_blocks)
            .with_split_on_write_unit(true)
            .with_split_on_optimal_io_bounary(true)
            .build_with_context_in_place(async move |mut ctx| {
                let this = ctx.write(Self {
                    workers: Vec::with_capacity(child_bdevs.len()),
                });

                for child_bdev in child_bdevs {
                    let thread = Raid5RsModule::instance().ctx().get_worker().await;

                    let child_desc = child_bdev.claim(
                        ReadManyWriteOne(c"Raid5Rs"),
                        Raid5RsModule::instance(),
                        &this,
                    )?;

                    this.workers
                        .push(Raid5RsWorker::new(thread, child_desc).await);
                }

                Ok(())
            })
            .await
    }

    fn get_worker_idx(&self, io: &IoRequest<Raid5RsIo>, stripe_blocks: u32) -> usize {
        (io.offset_blocks() / stripe_blocks as u64) as usize % self.workers.len()
    }

    fn get_worker_for_io(
        &self,
        io: &IoRequest<Raid5RsIo>,
        stripe_blocks: u32,
    ) -> &'child Raid5RsWorker<'_> {
        let worker_idx = self.get_worker_idx(io, stripe_blocks);

        &self.workers[worker_idx]
    }
}

unsafe impl Send for Raid5Rs<'_> {}
unsafe impl Sync for Raid5Rs<'_> {}

impl<'child> AsRef<Raid5Rs<'child>> for Raid5Rs<'child> {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<'child> BDevOps for Raid5Rs<'child> {
    type ChannelContext = ();

    type IoContext = Raid5RsIo;

    type IoError = IoError;

    async fn destruct(&mut self) -> spdk::Result<()> {
        for worker in &self.workers {
            worker.shutdown().await;
        }
        Ok(())
    }

    fn io_type_supported(&self, io_type: spdk::bdev::IoType) -> bool {
        matches!(io_type, IoType::Read | IoType::Write)
    }

    fn new_io_channel(&mut self) -> spdk::Result<Self::ChannelContext> {
        Ok(())
    }

    async fn handle_io<'io>(
        this: &'io BDevImpl<Self>,
        _channel: &'io IoChannelImpl<Self::ChannelContext>,
        io: &'io mut IoRequest<Self::IoContext>,
    ) -> IoResult<()> {
        // SAFETY: We "extend" the inferred child BDev lifetime parameter of the `Raid5Rs` structure
        // to match the I/O lifetime since child BDev will live for the processing duration of this
        // I/O request.
        let this = unsafe { transmute::<&BDevImpl<Raid5Rs<'_>>, &BDevImpl<Raid5Rs<'io>>>(this) };

        let bdev = this.as_bdev();
        let worker = this.ctx().get_worker_for_io(io, bdev.optimal_io_boundary());

        worker.process_io(io).await
    }
}

impl EventHandler for Raid5Rs<'_> {
    fn handle_event(&self, event: EventType, device: &BDev<OwnedBy<'_, Self>>) {
        // TODO: Add event handling
    }
}

#[derive(Debug, Default)]
struct Raid5RsModuleInner {
    workers: Vec<Thread<thread::Owned>>,
    next_worker: usize,
    bdevs: HashMap<CString, BDev<Owned>>,
}

impl Raid5RsModuleInner {
    fn init(&mut self) {
        runtime::reactors().for_each(|r| {
            let core = r.core();
            let worker_name = CString::new(format!("Raid5Rs Disk Worker {}", core.id()))
                .expect("worker name created");
            let worker = Thread::new(worker_name.as_c_str(), &core.into()).expect("worker created");

            self.workers.push(worker);
        });
    }

    fn fini(&mut self) {
        self.workers.clear();
    }
}

#[derive(Debug, Default)]
#[spdk::module(product_name = "Raid5Rs Disk")]
struct Raid5RsModule {
    inner: ThreadCell<App, Raid5RsModuleInner>,
}

impl Raid5RsModule {
    async fn create_bdev(
        &self,
        name: &str,
        stripe_size: u32,
        children: Vec<&'_ str>,
    ) -> spdk::Result<()> {
        debug_assert!(
            thread::is_application(),
            "create_bdev must be called on the application thread"
        );

        let name = CString::new(name).or(Err(EINVAL))?;
        let raid5rs = Raid5Rs::create_bdev(name.as_c_str(), stripe_size, children).await?;
        let inner_fn = move |inner: &mut Raid5RsModuleInner| {
            inner.bdevs.insert(name, raid5rs);
        };

        unsafe { self.inner.with_unchecked(inner_fn) };

        Ok(())
    }

    async fn destroy_bdev(&self, name: &str) -> spdk::Result<()> {
        debug_assert!(
            thread::is_application(),
            "create_bdev must be called on the application thread"
        );

        let name = CString::new(name).or(Err(EINVAL))?;
        let inner_fn = move |inner: &mut Raid5RsModuleInner| inner.bdevs.remove(&name);

        if let Some(raid5rs) = unsafe { self.inner.with_unchecked(inner_fn) } {
            return raid5rs.destroy().await;
        }

        Ok(())
    }

    async fn get_worker(&'static self) -> Thread<thread::OwnedBy<'static, Self>> {
        self.inner
            .with(|| {
                |inner| {
                    let worker = inner.next_worker;

                    inner.next_worker += 1;
                    if inner.next_worker >= inner.workers.len() {
                        inner.next_worker = 0;
                    }

                    unsafe { Thread::with_owner(self, &inner.workers[worker]) }
                }
            })
            .await
    }
}

impl ModuleOps for Raid5RsModule {
    type BDev<'bdev> = Raid5Rs<'bdev>;

    async fn init(&self) {
        debug_assert!(
            thread::is_application(),
            "init must be called on the application thread"
        );

        unsafe { self.inner.with_unchecked(Raid5RsModuleInner::init) };
    }

    async fn fini(&self) {
        debug_assert!(
            thread::is_application(),
            "fini must be called on the application thread"
        );

        unsafe { self.inner.with_unchecked(Raid5RsModuleInner::fini) };
    }
}

#[spdk::jsonrpc]
#[allow(clippy::needless_lifetimes)]
async fn create_raid5rs_bdev<'call>(
    name: &str,
    stripe_size: u32,
    children: Vec<&'call str>,
) -> spdk::Result<()> {
    Raid5RsModule::instance()
        .ctx()
        .create_bdev(name, stripe_size, children)
        .await
}

#[spdk::jsonrpc]
async fn destroy_raid5rs_bdev(name: &str) -> spdk::Result<()> {
    Raid5RsModule::instance().ctx().destroy_bdev(name).await
}

#[spdk::main]
async fn main() {
    runtime::wait_for_shutdown().await;
}
