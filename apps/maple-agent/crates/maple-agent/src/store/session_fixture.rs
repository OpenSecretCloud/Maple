//! Benchmark-only batching around the actual append primitives. Preparation
//! and simulated state changes happen outside the writer; live adapters change
//! only after the entire Durable transaction commits.

use super::*;
use crate::store::bench::{SeedEntry, SourceWriter};
use std::collections::HashMap;

struct Prepared {
    writer: usize,
    seq: i64,
    entry: SessionEntry,
    images: PreparedEntryImages,
    prepared: PreparedEntry,
    facts: AppendFacts,
    count: u64,
    base: Option<String>,
}

pub(in crate::store) fn seed_batch(
    store: &Store,
    writers: &mut [SourceWriter],
    inputs: &[SeedEntry],
) -> StoreResult<()> {
    if inputs.is_empty() || inputs.len() > 1000 {
        return Err(StoreError::InvalidInput);
    }
    let mut working = HashMap::new();
    let mut prepared = Vec::with_capacity(inputs.len());
    for input in inputs {
        // Images have their own digest-locked, chunked fixture scenario. This
        // text/source batch must never acquire one digest twice before commit.
        let image = match &input.entry.kind {
            EntryKind::Message {
                message: SessionMessage::Llm(Message::User(user)),
            } => user
                .content
                .iter()
                .any(|content| matches!(content, pi_ai::Content::Image(_))),
            EntryKind::Message {
                message: SessionMessage::Llm(Message::ToolResult(result)),
            } => result
                .content
                .iter()
                .any(|content| matches!(content, pi_ai::Content::Image(_))),
            _ => false,
        };
        if image {
            return Err(StoreError::InvalidInput);
        }
        let source = writers.get(input.writer).ok_or(StoreError::InvalidInput)?;
        let adapter = working
            .entry(input.writer)
            .or_insert_with(|| SqliteSessionStore {
                store: Arc::clone(&source.adapter.store),
                account: source.adapter.account,
                uuid: source.adapter.uuid.clone(),
                task: source.adapter.task,
                header: source.adapter.header.clone(),
                state: source.adapter.state.clone(),
                receipts: source.adapter.receipts.clone(),
            });
        if !Arc::ptr_eq(&adapter.store, &source.adapter.store)
            || !std::ptr::eq(adapter.store.as_ref(), store)
        {
            return Err(StoreError::InvalidInput);
        }
        if input.entry.parent_id.as_deref()
            != adapter.state.entries.last().map(|entry| entry.id.as_str())
        {
            return Err(StoreError::InvalidInput);
        }
        if adapter.header.is_none() {
            if !conversation(&input.entry) {
                return Err(StoreError::InvalidInput);
            }
            adapter.header = Some(source.header.clone());
        }
        let seq = adapter.state.last_seq + 1;
        let (images, value) = adapter.prepare(&input.entry, seq)?;
        let context_base = adapter.compaction_base(&input.entry, None)?;
        let cache = adapter.state.next_cache(&input.entry);
        let count = adapter.state.message_count + u64::from(conversation(&input.entry));
        prepared.push(Prepared {
            writer: input.writer,
            seq,
            entry: input.entry.clone(),
            images,
            prepared: value,
            facts: AppendFacts::of(&cache),
            count,
            base: context_base
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| StoreError::InvalidInput)?,
        });
        adapter.state.push(input.entry.clone(), cache, count, seq);
        adapter.trim_compacted(&input.entry, context_base);
    }
    store.write(Durability::Durable, |tx| {
        for (&index, adapter) in &working {
            adapter.guard(tx)?;
            let original = &writers[index].adapter;
            let last: i64 = tx.query_row(
                "SELECT last_seq FROM tasks WHERE id=?1",
                [adapter.task.0],
                |row| row.get(0),
            )?;
            if last != original.state.last_seq {
                return Err(StoreError::Conflict);
            }
            if original.header.is_none() {
                let header = serde_json::to_string(&writers[index].header)
                    .map_err(|_| StoreError::InvalidInput)?;
                tx.execute(
                    "UPDATE tasks SET pi_header=?2 WHERE id=?1 AND pi_header IS NULL",
                    params![adapter.task.0, header],
                )?;
            }
        }
        // Preserve the global timestamp order across tasks in the actual rowid
        // insertion order. All real source, cache and projector writes run here.
        for value in &prepared {
            let adapter = &working[&value.writer];
            append_tx(
                tx,
                adapter.task,
                value.seq,
                AppendInput {
                    entry: &value.entry,
                    images: &value.images,
                    prepared: &value.prepared,
                    cache: &value.facts,
                    count: value.count,
                    base: value.base.as_deref(),
                },
            )?;
        }
        Ok(())
    })?;
    for (index, adapter) in working {
        writers[index].adapter = adapter;
        writers[index].initialized = true;
    }
    Ok(())
}
