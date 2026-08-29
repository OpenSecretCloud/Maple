//! Run a backend future and apply its result to a view.
//!
//! Screens do the same dance for every backend call: spawn the future on
//! the backend runtime, await the join handle on the UI executor, then
//! update the entity with the result. This is that dance, written once.

use std::future::Future;

use gpui::Context;

use crate::backend::AgentBackend;

/// Run `future` on the backend runtime and hand its result to `then` on
/// the UI thread. A cancelled or panicked task becomes an `Err` so the
/// view always leaves its busy state.
pub fn call<V, T, F>(
    backend: &AgentBackend,
    future: F,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Result<T, String>, &mut Context<V>) + 'static,
) where
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
    .detach();
}
