//! A simple crate to open a document picker on iOS.

#![warn(missing_docs)]

use objc::{rc::StrongPtr, runtime::Object};
use pin_project::pin_project;
use stable_deref_trait::{CloneStableDeref, StableDeref};
use std::{
    ffi::{c_char, c_void, CString},
    fmt::Debug,
    future::Future,
    ops::Deref,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{oneshot, watch};
use tokio_stream::{wrappers::WatchStream, Stream, StreamExt};

#[link(name = "UIKit", kind = "framework")]
#[link(name = "UniformTypeIdentifiers", kind = "framework")]
#[link(name = "picker", kind = "static")]
extern "C" {
    fn show_browser(
        controller: *mut Object,
        extensions: *const *const c_char,
        types_len: usize,
        allow_multiple: bool,
        closure: unsafe extern "C" fn(*const c_void, usize, *mut c_void),
        closure_data: *mut c_void,
    ) -> *mut Object;
}

/// A file handle which contains the content of the file.
#[derive(Debug, Clone)]
pub struct FileHandle(Arc<[u8]>);

impl Deref for FileHandle {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

// SAFETY: Vec
unsafe impl StableDeref for FileHandle {}
unsafe impl CloneStableDeref for FileHandle {}

fn with_extension_ptrs<T>(extensions: &[&str], f: impl FnOnce(&[*const c_char]) -> T) -> T {
    let extensions = extensions
        .iter()
        .map(|s| CString::new(*s).unwrap())
        .collect::<Vec<_>>();
    let extension_ptrs = extensions.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
    f(&extension_ptrs)
}

unsafe extern "C" fn pick_file_closure(data: *const c_void, len: usize, closure_data: *mut c_void) {
    let sender = Box::from_raw(closure_data as *mut oneshot::Sender<Option<FileHandle>>);
    if !data.is_null() {
        let file_handle = FileHandle(std::slice::from_raw_parts(data as *const u8, len).into());
        sender.send(Some(file_handle)).ok();
    } else {
        sender.send(None).ok();
    }
}

/// Pick one file.
pub fn pick_file(
    controller: *mut Object,
    extensions: &[&str],
) -> impl Future<Output = Option<FileHandle>> + Send + Sync {
    with_extension_ptrs(extensions, |extension_ptrs| {
        let (tx, rx) = oneshot::channel::<Option<FileHandle>>();
        let delegate = unsafe {
            StrongPtr::retain(show_browser(
                controller,
                extension_ptrs.as_ptr(),
                extension_ptrs.len(),
                false,
                pick_file_closure,
                Box::into_raw(Box::new(tx)) as *mut _ as *mut c_void,
            ))
        };
        let f = async move {
            match rx.await {
                Ok(res) => res,
                Err(_) => None,
            }
        };
        PickFileFuture { f, delegate }
    })
}

#[pin_project]
struct PickFileFuture<F: Future<Output = Option<FileHandle>> + Send + Sync> {
    #[pin]
    f: F,
    delegate: StrongPtr,
}

unsafe impl<F: Future<Output = Option<FileHandle>> + Send + Sync> Send for PickFileFuture<F> {}
unsafe impl<F: Future<Output = Option<FileHandle>> + Send + Sync> Sync for PickFileFuture<F> {}

impl<F: Future<Output = Option<FileHandle>> + Send + Sync> Future for PickFileFuture<F> {
    type Output = Option<FileHandle>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().f.poll(cx)
    }
}

unsafe extern "C" fn pick_files_closure(
    data: *const c_void,
    len: usize,
    closure_data: *mut c_void,
) {
    let sender = Box::from_raw(closure_data as *mut watch::Sender<Option<FileHandle>>);
    if !data.is_null() {
        let file_handle = FileHandle(std::slice::from_raw_parts(data as *const u8, len).into());
        sender.send(Some(file_handle)).ok();
        std::mem::forget(sender);
    }
}

/// Pick multiple files.
///
/// If the picker is cancelled, the stream will be empty.
pub fn pick_files(
    controller: *mut Object,
    extensions: &[&str],
) -> impl Stream<Item = FileHandle> + Send + Sync {
    with_extension_ptrs(extensions, |extension_ptrs| {
        let (tx, rx) = watch::channel(None::<FileHandle>);
        let delegate = unsafe {
            StrongPtr::retain(show_browser(
                controller,
                extension_ptrs.as_ptr(),
                extension_ptrs.len(),
                true,
                pick_files_closure,
                Box::into_raw(Box::new(tx)) as *mut _ as *mut c_void,
            ))
        };
        let s = WatchStream::new(rx).filter_map(|f| f);
        PickFilesStream { s, delegate }
    })
}

#[pin_project]
struct PickFilesStream<S: Stream<Item = FileHandle> + Send + Sync> {
    #[pin]
    s: S,
    delegate: StrongPtr,
}

unsafe impl<S: Stream<Item = FileHandle> + Send + Sync> Send for PickFilesStream<S> {}
unsafe impl<S: Stream<Item = FileHandle> + Send + Sync> Sync for PickFilesStream<S> {}

impl<S: Stream<Item = FileHandle> + Send + Sync> Stream for PickFilesStream<S> {
    type Item = FileHandle;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().s.poll_next(cx)
    }
}
