//! Metadata-only resolution for streamed source occurrences and external repaints.

use rusqlite::{OptionalExtension, params};

use super::{AccountId, Store, StoreError, StoreResult, StoredRowChange, tasks::task_key_tx};

pub(crate) struct LiveOriginResolution {
    pub(crate) entry_id: String,
    pub(crate) item_index: i64,
    pub(crate) row: Option<StoredRowChange>,
    pub(crate) hidden: bool,
    pub(crate) known: bool,
}

impl Store {
    /// The same first-visible merge target the projector selects for an external
    /// agent repaint. This is deliberately not a general message identity lookup:
    /// ordinary live messages reconcile by source entry id and operation ordinal.
    pub(crate) fn external_repaint_target(
        &self,
        account: AccountId,
        task: &str,
        key: &str,
    ) -> StoreResult<Option<StoredRowChange>> {
        if key.is_empty() || key.len() > 1024 * 1024 {
            return Err(StoreError::InvalidInput);
        }
        self.read(|tx| {
            let task = task_key_tx(tx, account, task)?;
            Ok(tx.query_row(
                "SELECT r.epoch,r.row_key,r.row_seq,r.entry_seq,r.item_index,r.update_seq,c.entry_id,u.entry_id
                 FROM timeline_rows r JOIN tasks t ON t.id=r.task_id AND t.rows_epoch=r.epoch
                 JOIN entries c ON c.task_id=r.task_id AND c.seq=r.entry_seq
                 JOIN entries u ON u.task_id=r.task_id AND u.seq=r.update_seq
                 WHERE r.task_id=?1 AND r.row_key=?2 AND r.hidden=0 ORDER BY r.row_seq LIMIT 1",
                params![task.0,key], read_change,
            ).optional()?)
        })
    }

    /// Repair a lagged commit notification by exact creation origins, never by a
    /// legacy timestamp key. No source payload is decoded or copied.
    pub(crate) fn resolve_live_origins(
        &self,
        account: AccountId,
        task: &str,
        origins: &[(String, i64)],
    ) -> StoreResult<Vec<LiveOriginResolution>> {
        if origins.len() > 500
            || origins
                .iter()
                .any(|(id, index)| id.is_empty() || id.len() > 128 || *index < 0)
        {
            return Err(StoreError::InvalidInput);
        }
        self.read(|tx| {
            let task = task_key_tx(tx, account, task)?;
            let mut result = Vec::with_capacity(origins.len());
            for (entry_id, item_index) in origins {
                let source = tx.query_row("SELECT seq FROM entries WHERE task_id=?1 AND entry_id=?2",
                    params![task.0,entry_id], |row|row.get::<_,i64>(0)).optional()?;
                let mut selected = match source {
                    Some(source) => tx.query_row(
                        "SELECT r.epoch,r.row_key,r.row_seq,r.entry_seq,r.item_index,r.update_seq,c.entry_id,u.entry_id,r.hidden
                         FROM timeline_rows r JOIN tasks t ON t.id=r.task_id AND t.rows_epoch=r.epoch
                         JOIN entries c ON c.task_id=r.task_id AND c.seq=r.entry_seq
                         JOIN entries u ON u.task_id=r.task_id AND u.seq=r.update_seq
                         WHERE r.task_id=?1 AND r.entry_seq=?2 AND r.item_index=?3 ORDER BY r.row_seq LIMIT 1",
                        params![task.0,source,item_index], |row|Ok((read_change(row)?,row.get::<_,bool>(8)?)),
                    ).optional()?,
                    None => None,
                };
                if selected.is_none() && *item_index == 0 && let Some(source) = source {
                    // One tool-result/external repaint operation can update a
                    // row created in an unloaded prefix. Resolve that exact
                    // update source only when its target is unambiguous.
                    let mut statement = tx.prepare(
                        "SELECT r.epoch,r.row_key,r.row_seq,r.entry_seq,r.item_index,r.update_seq,c.entry_id,u.entry_id,r.hidden
                         FROM timeline_rows r JOIN tasks t ON t.id=r.task_id AND t.rows_epoch=r.epoch
                         JOIN entries c ON c.task_id=r.task_id AND c.seq=r.entry_seq
                         JOIN entries u ON u.task_id=r.task_id AND u.seq=r.update_seq
                         WHERE r.task_id=?1 AND r.update_seq=?2 AND r.entry_seq<>?2 ORDER BY r.row_seq LIMIT 2")?;
                    let mut matches = statement.query_map(params![task.0,source], |row|Ok((read_change(row)?,row.get::<_,bool>(8)?)))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    if matches.len() == 1 { selected = matches.pop(); }
                }
                let known = source.is_none() || selected.is_some();
                let (row, hidden) = match selected { Some((row,false)) => (Some(row),false), Some((_,true)) => (None,true), None => (None,false) };
                result.push(LiveOriginResolution { entry_id: entry_id.clone(), item_index: *item_index, row, hidden, known });
            }
            Ok(result)
        })
    }
}

fn read_change(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRowChange> {
    Ok(StoredRowChange {
        epoch: row.get(0)?,
        row_key: row.get(1)?,
        row_seq: row.get(2)?,
        entry_seq: row.get(3)?,
        item_index: row.get(4)?,
        update_seq: row.get(5)?,
        origin_entry_id: row.get(6)?,
        update_entry_id: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        StorePaths,
        session::SessionLoad,
        tasks::{NewTask, TaskKind},
    };
    use pi_ai::{Content, Message, UserMessage};
    use pi_coding_agent::{SessionMessage, session::SessionManager};
    use std::sync::Arc;

    const TASK: &str = "00000000-0000-7000-8000-000000000012";

    fn fixture() -> (tempfile::TempDir, Arc<Store>, AccountId, SessionManager) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(StorePaths::new(directory.path())).unwrap());
        let account = store.upsert_account("identity@example.test", 1).unwrap();
        store
            .create_task(
                account,
                &NewTask {
                    uuid: TASK.into(),
                    project_root: if cfg!(windows) {
                        "C:/identity"
                    } else {
                        "/identity"
                    }
                    .into(),
                    kind: TaskKind::Desktop,
                    title: "Identity".into(),
                    provisional: false,
                    model: None,
                    created_ms: 1,
                },
            )
            .unwrap();
        let manager = store
            .load_session(account, TASK, SessionLoad::Context)
            .unwrap()
            .into_manager("/identity");
        (directory, store, account, manager)
    }

    fn append(manager: &mut SessionManager, text: &str) -> String {
        manager.append_message(SessionMessage::Llm(Message::User(UserMessage {
            content: vec![Content::text(text)],
            timestamp: 7,
        })))
    }

    #[test]
    fn exact_origins_keep_equal_legacy_keys_distinct_and_repaint_uses_projector_target() {
        let (_directory, store, account, mut manager) = fixture();
        let first = append(&mut manager, "first at seven");
        let second = append(&mut manager, "second at seven");
        manager.flush().unwrap();
        let rows = store
            .resolve_live_origins(
                account,
                TASK,
                &[
                    (first.clone(), 0),
                    (second.clone(), 0),
                    ("not-yet-written".into(), 0),
                ],
            )
            .unwrap();
        let a = rows[0].row.as_ref().unwrap();
        let b = rows[1].row.as_ref().unwrap();
        assert_eq!(a.row_key, b.row_key);
        assert_ne!(a.row_seq, b.row_seq);
        assert_eq!(a.origin_entry_id, first);
        assert_eq!(b.origin_entry_id, second);
        assert!(rows[2].known && rows[2].row.is_none() && !rows[2].hidden);
        assert_eq!(
            store
                .external_repaint_target(account, TASK, &a.row_key)
                .unwrap()
                .unwrap()
                .row_seq,
            a.row_seq
        );
        store
            .write(super::super::Durability::Durable, |tx| {
                let task = task_key_tx(tx, account, TASK)?;
                tx.execute(
                    "UPDATE timeline_rows SET hidden=1 WHERE task_id=?1 AND row_seq=?2",
                    params![task.0, a.row_seq],
                )?;
                Ok(())
            })
            .unwrap();
        let hidden = store
            .resolve_live_origins(account, TASK, &[(first, 0), (second, 0)])
            .unwrap();
        assert!(hidden[0].known && hidden[0].hidden && hidden[0].row.is_none());
        assert_eq!(
            store
                .external_repaint_target(account, TASK, &a.row_key)
                .unwrap()
                .unwrap()
                .row_seq,
            b.row_seq
        );
    }

    #[test]
    fn known_source_without_current_projection_requires_resync_and_inputs_are_bounded() {
        let (_directory, store, account, mut manager) = fixture();
        let entry = append(&mut manager, "visible");
        manager.flush().unwrap();
        let missing_operation = store
            .resolve_live_origins(account, TASK, &[(entry, 900)])
            .unwrap();
        assert!(!missing_operation[0].known);
        assert!(
            store
                .resolve_live_origins(account, TASK, &vec![("entry".into(), 0); 501])
                .is_err()
        );
        assert!(
            store
                .resolve_live_origins(account, TASK, &[("entry".into(), -1)])
                .is_err()
        );
    }

    #[test]
    fn a_result_update_resolves_its_actual_prefix_creation_origin() {
        let (_directory, store, account, mut manager) = fixture();
        append(
            &mut manager,
            "Start the conversation before tool results are persistable",
        );
        let result = |text: &str| {
            SessionMessage::Llm(Message::ToolResult(pi_ai::ToolResultMessage {
                tool_call_id: "old-call".into(),
                tool_name: "read".into(),
                content: vec![Content::text(text)],
                details: None,
                usage: None,
                is_error: false,
                timestamp: 7,
            }))
        };
        let creation = manager.append_message(result("first"));
        let update = manager.append_message(result("second"));
        manager.flush().unwrap();
        let resolved = store
            .resolve_live_origins(account, TASK, &[(update.clone(), 0)])
            .unwrap();
        let row = resolved[0].row.as_ref().unwrap();
        assert_eq!(row.origin_entry_id, creation);
        assert_eq!(row.update_entry_id, update);
    }
}
