//! Run a backend future and apply its result to a view.
//!
//! Screens do the same dance for every backend call: spawn the future on
//! the backend runtime, await the join handle on the UI executor, then
//! update the entity with the result. This is that dance, written once.

use std::future::Future;

use gpui::{Context, Task};

use crate::backend::AgentBackend;

/// Keep a bridging task alive on its view. Finished bridges are pruned
/// first, so the vector stays bounded by the number of calls in flight
/// rather than growing forever. Dropping a live task would cancel it,
/// which is why nothing here evicts by age.
pub fn retain(tasks: &std::cell::RefCell<Vec<Task<()>>>, bridge: Task<()>) {
    let mut tasks = tasks.borrow_mut();
    if tasks.len() >= 16 {
        tasks.retain(|task| !task.is_ready());
    }
    tasks.push(bridge);
}

/// Run `future` on the backend runtime and hand its result to `then` on
/// the UI thread. A cancelled or panicked task becomes an `Err` so the
/// view always leaves its busy state.
///
/// Returns the bridging task instead of detaching it: the backend's
/// sender holds the task's waker, and this gpui revision asserts a task
/// is only dropped by the thread that spawned it. Retaining the task on
/// the owning view (see [`retain`]) keeps the final drop on the UI (or
/// test) thread.
pub fn call<V, T, F>(
    backend: &AgentBackend,
    future: F,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Result<T, String>, &mut Context<V>) + 'static,
) -> Task<()>
where
    V: 'static,
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let task = backend.spawn(future);
    cx.spawn(async move |this, cx| {
        let result = task.await.unwrap_or_else(|error| {
            log::debug!("backend task failed: {error:?}");
            Err("The task was cancelled".to_string())
        });
        this.update(cx, |this, cx| then(this, result, cx)).ok();
    })
}
