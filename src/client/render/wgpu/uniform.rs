use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    mem::{align_of, size_of},
    rc::Rc,
};

use crate::common::util::{any_as_bytes, Pod};

use failure::Error;

// https://www.khronos.org/registry/vulkan/specs/1.2-extensions/html/vkspec.html#limits-maxUniformBufferRange
const DYNAMIC_UNIFORM_BUFFER_SIZE: wgpu::BufferAddress = 16384;

// https://www.khronos.org/registry/vulkan/specs/1.2-extensions/html/vkspec.html#limits-minUniformBufferOffsetAlignment
pub const DYNAMIC_UNIFORM_BUFFER_ALIGNMENT: usize = 256;

/// A handle to a dynamic uniform buffer on the GPU.
///
/// Allows allocation and updating of individual blocks of memory.
pub struct DynamicUniformBuffer<'a, T>
where
    T: Pod,
{
    // keeps track of how many blocks are allocated so we know whether we can
    // clear the buffer or not
    _rc: RefCell<Rc<()>>,

    // represents the data in the buffer, which we don't actually own
    _phantom: PhantomData<&'a [T]>,

    inner: wgpu::Buffer,
    allocated: Cell<wgpu::BufferSize>,
    update_buf: Vec<u8>,
}

impl<'a, T> DynamicUniformBuffer<'a, T>
where
    T: Pod,
{
    pub fn new<'b>(device: &'b wgpu::Device) -> DynamicUniformBuffer<'a, T> {
        // TODO: is this something we can enforce at compile time?
        assert!(align_of::<T>() % DYNAMIC_UNIFORM_BUFFER_ALIGNMENT == 0);

        let inner = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dynamic uniform buffer"),
            size: DYNAMIC_UNIFORM_BUFFER_SIZE,
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });

        let mut update_buf = Vec::with_capacity(DYNAMIC_UNIFORM_BUFFER_SIZE as usize);
        update_buf.resize(DYNAMIC_UNIFORM_BUFFER_SIZE as usize, 0);

        DynamicUniformBuffer {
            _rc: RefCell::new(Rc::new(())),
            _phantom: PhantomData,
            inner,
            allocated: Cell::new(wgpu::BufferSize(0)),
            update_buf,
        }
    }

    pub fn block_size(&self) -> wgpu::BufferSize {
        wgpu::BufferSize((DYNAMIC_UNIFORM_BUFFER_ALIGNMENT.max(size_of::<T>())) as u64)
    }

    /// Allocates a block of memory in this dynamic uniform buffer with the
    /// specified initial value.
    #[must_use]
    pub fn allocate(&mut self, val: T) -> DynamicUniformBufferBlock<'a, T> {
        trace!("Allocating dynamic uniform block");
        let allocated = self.allocated.get().0;
        let size = self.block_size().0;
        if allocated + size > DYNAMIC_UNIFORM_BUFFER_SIZE {
            panic!(
                "Not enough space to allocate {} bytes in dynamic uniform buffer",
                size
            );
        }

        let addr = allocated;
        self.allocated.set(wgpu::BufferSize(allocated + size));

        let block = DynamicUniformBufferBlock {
            _rc: self._rc.borrow().clone(),
            _phantom: PhantomData,
            addr,
        };

        self.write_block(&block, val);
        block
    }

    pub fn write_block(&mut self, block: &DynamicUniformBufferBlock<'a, T>, val: T) {
        let start = block.addr as usize;
        let end = start + self.block_size().0 as usize;
        let mut slice = &mut self.update_buf[start..end];
        slice.copy_from_slice(unsafe { any_as_bytes(&val) });
    }

    /// Removes all allocations from the underlying buffer.
    ///
    /// Returns an error if the buffer is currently mapped or there are
    /// outstanding allocated blocks.
    pub fn clear(&self) -> Result<(), Error> {
        let mut out = self._rc.replace(Rc::new(()));
        match Rc::try_unwrap(out) {
            // no outstanding blocks
            Ok(()) => {
                self.allocated.set(wgpu::BufferSize(0));
                Ok(())
            }
            Err(rc) => {
                let _ = self._rc.replace(rc);
                bail!("Can't clear uniform buffer: there are outstanding references to allocated blocks.");
            }
        }
    }

    pub fn flush(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.inner, 0, &self.update_buf);
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.inner
    }
}

/// An address into a dynamic uniform buffer.
#[derive(Debug)]
pub struct DynamicUniformBufferBlock<'a, T> {
    _rc: Rc<()>,
    _phantom: PhantomData<&'a T>,

    addr: wgpu::BufferAddress,
}

impl<'a, T> DynamicUniformBufferBlock<'a, T> {
    pub fn offset(&self) -> wgpu::DynamicOffset {
        self.addr as wgpu::DynamicOffset
    }
}