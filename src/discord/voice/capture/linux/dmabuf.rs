use std::{io, os::fd::RawFd, ptr::NonNull};

use crate::logging;

#[path = "egl.rs"]
mod egl;
pub(super) use egl::{DmaBufPlane, EglDmaBufImporter};

pub(super) const DRM_FORMAT_MOD_LINEAR: u64 = 0;
const DMA_BUF_SYNC_READ: u64 = 1 << 0;
const DMA_BUF_SYNC_START: u64 = 0 << 2;
const DMA_BUF_SYNC_END: u64 = 1 << 2;
const DMA_BUF_READY_TIMEOUT_MS: libc::c_int = 1_000;
const DMA_BUF_IOCTL_TYPE: u32 = b'b' as u32;

pub(super) struct DmaBufMapping {
    pointer: NonNull<u8>,
    length: usize,
    staging: Vec<u8>,
}

impl DmaBufMapping {
    pub(super) fn new(fd: RawFd) -> Result<Self, String> {
        let length = dma_buf_length(fd)?;
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return Err(format!(
                "PipeWire linear DMA-BUF could not be mapped for CPU conversion: {}",
                io::Error::last_os_error()
            ));
        }
        let Some(pointer) = NonNull::new(pointer.cast::<u8>()) else {
            let _ = unsafe { libc::munmap(pointer, length) };
            return Err("PipeWire linear DMA-BUF mapping returned a null pointer".to_owned());
        };
        Ok(Self {
            pointer,
            length,
            staging: Vec::new(),
        })
    }

    /// Copies the mapping into system memory for the pixel conversion.
    ///
    /// A DMA-BUF maps as uncached GPU memory, so the per-pixel reads the
    /// conversion does cost hundreds of nanoseconds each: on radeonsi a single
    /// 2560x1440 frame takes seconds that way. Those reads happen inside the
    /// PipeWire `process` callback while it holds a dequeued buffer, so they
    /// stall the capture loop and starve the compositor of buffers until the
    /// readiness timeout gives up. One sequential bulk copy costs milliseconds.
    pub(super) fn refresh(&mut self) {
        let mapped = unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) };
        self.staging.resize(self.length, 0);
        self.staging.copy_from_slice(mapped);
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.staging
    }
}

impl Drop for DmaBufMapping {
    fn drop(&mut self) {
        if unsafe { libc::munmap(self.pointer.as_ptr().cast(), self.length) } != 0 {
            logging::debug(
                "stream",
                format!(
                    "PipeWire linear DMA-BUF unmap failed: {}",
                    io::Error::last_os_error()
                ),
            );
        }
    }
}

#[repr(C)]
struct DmaBufSync {
    flags: u64,
}

pub(super) struct DmaBufReadGuard {
    fd: RawFd,
    finished: bool,
}

impl DmaBufReadGuard {
    pub(super) fn begin(fd: RawFd) -> Result<Self, String> {
        wait_for_dma_buf(fd)?;
        dma_buf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ).map_err(|error| {
            format!("PipeWire DMA-BUF CPU read synchronization failed: {error}")
        })?;
        Ok(Self {
            fd,
            finished: false,
        })
    }

    fn finish(mut self) -> Result<(), String> {
        self.finished = true;
        dma_buf_sync(self.fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ)
            .map_err(|error| format!("PipeWire DMA-BUF CPU read completion failed: {error}"))
    }
}

impl Drop for DmaBufReadGuard {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = dma_buf_sync(self.fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ)
        {
            logging::debug(
                "stream",
                format!("PipeWire DMA-BUF CPU read cleanup failed: {error}"),
            );
        }
    }
}

pub(super) fn finish_dma_buf_reads(reads: Vec<DmaBufReadGuard>) -> Result<(), String> {
    let mut first_error = None;
    for read in reads {
        if let Err(error) = read.finish()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn dma_buf_length(fd: RawFd) -> Result<usize, String> {
    let length = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
    if length < 0 {
        return Err(format!(
            "PipeWire DMA-BUF size lookup failed: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::lseek(fd, 0, libc::SEEK_SET) } < 0 {
        return Err(format!(
            "PipeWire DMA-BUF offset reset failed: {}",
            io::Error::last_os_error()
        ));
    }
    let length =
        usize::try_from(length).map_err(|_| "PipeWire DMA-BUF size is too large".to_owned())?;
    if length == 0 {
        return Err("PipeWire DMA-BUF has zero length".to_owned());
    }
    if length > isize::MAX as usize {
        return Err("PipeWire DMA-BUF is too large to map safely".to_owned());
    }
    Ok(length)
}

fn wait_for_dma_buf(fd: RawFd) -> Result<(), String> {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let result = unsafe { libc::poll(&mut poll_fd, 1, DMA_BUF_READY_TIMEOUT_MS) };
        if result > 0 {
            if poll_fd.revents & libc::POLLNVAL != 0 {
                return Err("PipeWire DMA-BUF became invalid before CPU conversion".to_owned());
            }
            if poll_fd.revents & libc::POLLERR != 0 {
                return Err("PipeWire DMA-BUF reported an error before CPU conversion".to_owned());
            }
            if poll_fd.revents & libc::POLLIN == 0 {
                return Err(format!(
                    "PipeWire DMA-BUF readiness wait returned unexpected flags: {}",
                    poll_fd.revents
                ));
            }
            return Ok(());
        }
        if result == 0 {
            return Err("PipeWire DMA-BUF did not become ready for CPU conversion".to_owned());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(format!("PipeWire DMA-BUF readiness wait failed: {error}"));
    }
}

fn dma_buf_sync(fd: RawFd, flags: u64) -> io::Result<()> {
    let sync = DmaBufSync { flags };
    // `libc` selects both the target's ioctl encoding and its request type.
    // musl uses `c_int` here while glibc uses `c_ulong`, so a typed integer
    // constant is not portable between the two C libraries.
    let request = libc::_IOW::<DmaBufSync>(DMA_BUF_IOCTL_TYPE, 0);
    loop {
        if unsafe { libc::ioctl(fd, request, &sync) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN)
        {
            continue;
        }
        return Err(error);
    }
}
