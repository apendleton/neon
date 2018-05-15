//! A trait for defining Rust _tasks_ to be executed in a background thread.

use std::marker::{Send, Sized};
use std::mem;
use std::os::raw::c_void;

use js::{Value, JsFunction};
use mem::Handle;
use mem::Managed;
use vm::{Vm, JsResult};
use vm::internal::{VmInternal, Scope};
use neon_runtime;
use neon_runtime::raw;

pub struct TaskContext<'a> {
    scope: Scope<'a>
}

impl<'a> TaskContext<'a> {
    fn with<T, F: for<'b> FnOnce(TaskContext<'b>) -> T>(f: F) -> T {
        Scope::with(|scope| {
            f(TaskContext { scope })
        })
    }
}

impl<'a> VmInternal<'a> for TaskContext<'a> {
    fn scope(&self) -> &Scope<'a> {
        &self.scope
    }
}

impl<'a> Vm<'a> for TaskContext<'a> {

}

/// A Rust task that can be executed in a background thread.
pub trait Task: Send + Sized {
    /// The task's result type, which is sent back to the main thread to communicate a successful result back to JavaScript.
    type Output: Send;

    /// The task's error type, which is sent back to the main thread to communicate a task failure back to JavaScript.
    type Error: Send;

    /// The type of JavaScript value that gets produced to the asynchronous callback on the main thread after the task is completed.
    type JsEvent: Value;

    /// Perform the task, producing either a successful `Output` or an unsuccessful `Error`. This method is executed in a background thread as part of libuv's built-in thread pool.
    fn perform(&self) -> Result<Self::Output, Self::Error>;

    /// Convert the result of the task to a JavaScript value to be passed to the asynchronous callback. This method is executed on the main thread at some point after the background task is completed.
    fn complete<'a>(self, vm: TaskContext<'a>, result: Result<Self::Output, Self::Error>) -> JsResult<Self::JsEvent>;

    /// Schedule a task to be executed on a background thread.
    ///
    /// `callback` should have the following signature:
    ///
    /// ```js
    /// function callback(err, value) {}
    /// ```
    fn schedule(self, callback: Handle<JsFunction>) {
        let boxed_self = Box::new(self);
        let self_raw = Box::into_raw(boxed_self);
        let callback_raw = callback.to_raw();
        unsafe {
            neon_runtime::task::schedule(mem::transmute(self_raw),
                                         perform_task::<Self>,
                                         complete_task::<Self>,
                                         callback_raw);
        }
    }
}

unsafe extern "C" fn perform_task<T: Task>(task: *mut c_void) -> *mut c_void {
    let task: Box<T> = Box::from_raw(mem::transmute(task));
    let result = task.perform();
    Box::into_raw(task);
    mem::transmute(Box::into_raw(Box::new(result)))
}

unsafe extern "C" fn complete_task<T: Task>(task: *mut c_void, result: *mut c_void, out: &mut raw::Local) {
    let result: Result<T::Output, T::Error> = *Box::from_raw(mem::transmute(result));
    let task: Box<T> = Box::from_raw(mem::transmute(task));
    TaskContext::with(|vm| {
        if let Ok(result) = task.complete(vm, result) {
            *out = result.to_raw();
        }
    })
}
