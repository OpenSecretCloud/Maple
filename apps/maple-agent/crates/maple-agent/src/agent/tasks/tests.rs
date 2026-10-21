use super::*;

#[tokio::test]
async fn archive_project_scans_past_archived_pages_and_keeps_other_projects() {
    let harness = crate::agent::tests::Harness::new().await;
    let runtime = harness.runtime().await;
    let root = path_string(&normalize_project_root(harness.project.path()).unwrap());
    for index in 0..205 {
        let mut row = TaskRow::new(
            format!("10000000-0000-7000-8000-{index:012x}"),
            "Task".into(),
            root.clone(),
            TaskKind::Desktop,
            None,
            index + 1,
        );
        if index >= 5 {
            row.state = AgentTaskState::Archived;
        }
        runtime.store.insert(&row).unwrap();
    }
    let other = tempfile::tempdir().unwrap();
    let other_root = path_string(&normalize_project_root(other.path()).unwrap());
    let other_id = "20000000-0000-7000-8000-000000000001";
    runtime
        .store
        .insert(&TaskRow::new(
            other_id.into(),
            "Other".into(),
            other_root,
            TaskKind::Desktop,
            None,
            1,
        ))
        .unwrap();
    let first = runtime
        .store
        .store()
        .archive_project_candidates(runtime.store.account(), &root, None, 200)
        .unwrap();
    assert!(first.task_ids.is_empty());
    assert!(first.next.is_some());
    harness
        .handle
        .archive_project_root(root.clone(), None)
        .await
        .unwrap();
    for index in 0..205 {
        assert_eq!(
            runtime
                .store
                .get(&format!("10000000-0000-7000-8000-{index:012x}"))
                .unwrap()
                .unwrap()
                .state,
            AgentTaskState::Archived
        );
    }
    assert!(
        runtime
            .store
            .store()
            .project(runtime.store.account(), &root)
            .unwrap()
            .unwrap()
            .removed_ms
            .is_some()
    );
    assert_eq!(
        runtime.store.get(other_id).unwrap().unwrap().state,
        AgentTaskState::Active
    );
}

#[test]
fn a_prompt_title_is_one_shortened_line() {
    assert_eq!(
        session_title_from_prompt("  Fix\nthe   build "),
        "Fix the build"
    );
    let long = "word ".repeat(40);
    let title = session_title_from_prompt(&long);
    assert_eq!(title.chars().count(), MAX_AGENT_SESSION_TITLE_CHARS);
    assert!(title.ends_with('…'));
    assert!(!title.ends_with(" …"));
}

#[test]
fn user_titles_are_single_visible_lines() {
    assert_eq!(
        normalize_user_provided_session_title("  Release notes ").unwrap(),
        "Release notes"
    );
    assert!(normalize_user_provided_session_title("   ").is_err());
    assert!(normalize_user_provided_session_title("two\nlines").is_err());
    assert!(normalize_user_provided_session_title("\u{200b}\u{2060}").is_err());
    assert!(normalize_user_provided_session_title(&"x".repeat(81)).is_err());
}

#[test]
fn only_an_unnamed_unstarted_task_takes_its_title_from_the_prompt() {
    let mut row = TaskRow::new(
        "task".into(),
        DEFAULT_AGENT_SESSION_TITLE.into(),
        "/project".into(),
        TaskKind::Desktop,
        None,
        1,
    );
    assert!(names_from_prompt(&row));
    row.title = ACP_SESSION_FALLBACK_TITLE.into();
    assert!(names_from_prompt(&row));
    row.message_count = 2;
    assert!(!names_from_prompt(&row));
    row.message_count = 0;
    row.title = "Chosen".into();
    assert!(!names_from_prompt(&row));
    row.title = DEFAULT_AGENT_SESSION_TITLE.into();
    row.title_user_set = true;
    assert!(!names_from_prompt(&row));
}
