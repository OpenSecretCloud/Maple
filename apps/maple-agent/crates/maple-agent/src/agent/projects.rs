//! Project labels belong to the execution computer, independently of desktop
//! preferences and the account configuration that an OS profile may roam.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use icu_properties::{CodePointSetData, props::DefaultIgnorableCodePoint};
use serde::{Deserialize, Serialize};

use super::config::*;
use super::{AgentRuntimeHandle, AgentServiceEvent, RecentProjectRoot, emit_agent_event};

const MAX_NAME_CHARS: usize = 128;

#[derive(Default, Deserialize, Serialize)]
struct ProjectNames {
    #[serde(default)]
    legacy_imported: bool,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    names: BTreeMap<String, ProjectName>,
}

#[derive(Deserialize, Serialize)]
struct ProjectName {
    /// `None` is an explicit reset, not permission to import a legacy alias.
    name: Option<String>,
    revision: u64,
}

impl ProjectNames {
    fn load(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err("Could not read project display names".into()),
        };
        let names: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Project display names are invalid; restore the saved file".to_string())?;
        for (path, entry) in &names.names {
            if !structurally_valid_project_root(path)
                || entry.revision == 0
                || entry.revision > names.revision
                || entry
                    .name
                    .as_ref()
                    .is_some_and(|name| normalize_name(name).ok().as_ref() != Some(name))
            {
                return Err("Project display names are invalid; restore the saved file".into());
            }
        }
        Ok(names)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        write_device_local_json_file(path, self)
            .map_err(|_| "Could not save project display names".into())
    }

    fn set(&mut self, path: String, name: Option<String>) -> Result<bool, String> {
        if self
            .names
            .get(&path)
            .is_some_and(|entry| entry.name == name)
        {
            return Ok(false);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("Project display name revision is exhausted")?;
        self.names.insert(
            path,
            ProjectName {
                name,
                revision: self.revision,
            },
        );
        Ok(true)
    }

    fn apply(&self, roots: &mut [RecentProjectRoot], workspace: Option<&Path>) {
        for root in roots {
            root.is_default_workspace = workspace.is_some_and(|path| path == Path::new(&root.path));
            root.name = default_name(root);
            root.name_revision = 0;
            if let Some(entry) = self.names.get(&root.path) {
                root.name_revision = entry.revision;
                if let Some(name) = &entry.name {
                    root.name.clone_from(name);
                }
            }
        }
    }
}

fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "Project name must be {MAX_NAME_CHARS} characters or fewer"
        ));
    }
    if name
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
    {
        return Err("Project name must be a single line without control characters".into());
    }
    let invisible = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    if !name
        .chars()
        .any(|ch| !ch.is_whitespace() && !invisible.contains(ch))
    {
        return Err("Project name must contain visible characters".into());
    }
    Ok(name.into())
}

fn default_name(root: &RecentProjectRoot) -> String {
    if root.is_default_workspace {
        return super::MAPLE_WORKSPACE_DISPLAY_NAME.into();
    }
    project_root_record(root.path.clone(), root.last_used_ms).name
}

fn catalog(
    roots: Vec<RecentProjectRoot>,
    task_roots: impl IntoIterator<Item = String>,
    removed: &[String],
    workspace: Option<&Path>,
) -> Vec<RecentProjectRoot> {
    let mut roots = sanitize_recent_project_roots(roots);
    let mut known = roots
        .iter()
        .map(|root| root.path.clone())
        .collect::<HashSet<_>>();
    for path in task_roots {
        if structurally_valid_project_root(&path) && known.insert(path.clone()) {
            roots.push(project_root_record(path, 0));
        }
    }
    roots.retain(|root| !is_removed_project_root(&root.path, removed));
    include_default_maple_workspace(roots, removed, workspace)
}

impl AgentRuntimeHandle {
    fn project_names_path(&self) -> Result<PathBuf, String> {
        Ok(account_local_data_dir_path(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?
            .join("project_names.json"))
    }

    /// The caller holds the settings lock. Names never come from the roaming
    /// recent-roots cache; the owning daemon resolves both custom and defaults.
    pub(super) fn project_roots_locked(&self) -> Result<Vec<RecentProjectRoot>, String> {
        self.project_roots_with_removed_locked(false)
    }

    fn project_roots_with_removed_locked(
        &self,
        include_removed: bool,
    ) -> Result<Vec<RecentProjectRoot>, String> {
        let config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        let roots = load_recent_project_roots_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        let task_roots = self
            .store()?
            .list(None)?
            .into_iter()
            .map(|row| row.project_root);
        let workspace = super::default_maple_workspace_path();
        let mut roots = catalog(
            roots,
            task_roots,
            if include_removed {
                &[]
            } else {
                &config.removed_project_roots
            },
            workspace.as_deref(),
        );
        ProjectNames::load(&self.project_names_path()?)?.apply(&mut roots, workspace.as_deref());
        Ok(roots)
    }

    pub async fn list_recent_project_roots(&self) -> Result<Vec<RecentProjectRoot>, String> {
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        self.project_roots_locked()
    }

    /// A label mutation only: paths, project trust and tasks are unchanged.
    /// Typing the owner's default label resets a custom name.
    pub async fn rename_project_root(
        &self,
        path: String,
        name: String,
    ) -> Result<Vec<RecentProjectRoot>, String> {
        let name = normalize_name(&name)?;
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let roots = self.project_roots_locked()?;
        let root = roots
            .iter()
            .find(|root| root.path == path)
            .ok_or("This project is not in this computer's project list")?;
        let name = (name != default_name(root)).then_some(name);
        let file = self.project_names_path()?;
        let mut names = ProjectNames::load(&file)?;
        if !names.set(path, name)? {
            return Ok(roots);
        }
        names.save(&file)?;
        let mut roots = roots;
        names.apply(&mut roots, super::default_maple_workspace_path().as_deref());
        emit_agent_event(
            &self.service.host.events,
            AgentServiceEvent::ProjectsChanged {
                user_id: self.user_id.to_string(),
                roots: roots.clone(),
            },
        );
        Ok(roots)
    }

    /// Once imported, a desktop settings read is no longer needed, including
    /// after daemon restart. An unrelated UI settings error cannot undo that.
    pub async fn project_names_migrated(&self) -> Result<bool, String> {
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        Ok(ProjectNames::load(&self.project_names_path()?)?.legacy_imported)
    }

    /// Preserve legacy owner names without letting a later desktop save or
    /// repeated import undo a rename/reset already accepted by the daemon.
    /// Call only with the execution computer's map, never controller aliases.
    pub async fn migrate_project_names(
        &self,
        legacy: HashMap<String, String>,
    ) -> Result<(), String> {
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let file = self.project_names_path()?;
        let mut names = ProjectNames::load(&file)?;
        if names.legacy_imported {
            return Ok(());
        }
        let roots = self.project_roots_with_removed_locked(true)?;
        let mut changed = false;
        for root in &roots {
            if names.names.contains_key(&root.path) {
                continue;
            }
            if let Some(name) = legacy
                .get(&root.path)
                .and_then(|name| normalize_name(name).ok())
            {
                let name = (name != default_name(root)).then_some(name);
                changed |= names.set(root.path.clone(), name)?;
            }
        }
        names.legacy_imported = true;
        names.save(&file)?;
        if changed {
            let roots = self.project_roots_locked()?;
            emit_agent_event(
                &self.service.host.events,
                AgentServiceEvent::ProjectsChanged {
                    user_id: self.user_id.to_string(),
                    roots,
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent::store::{TaskKind, TaskRow};
    use crate::agent::tests::Recorder;
    use crate::agent::{AgentPathLayout, MapleAgentHostResources, MapleAgentService};

    const USER: &str = "project-names@example.com";

    fn service(config: &Path, data: &Path, recorder: Arc<Recorder>) -> MapleAgentService {
        MapleAgentService::new(MapleAgentHostResources::new(
            AgentPathLayout::from_app_roots(config.to_owned(), data.to_owned())
                .with_home(Some(data.join("home"))),
            recorder,
            "Project names fixture".into(),
        ))
    }

    async fn fixture() -> (
        tempfile::TempDir,
        MapleAgentService,
        AgentRuntimeHandle,
        Arc<Recorder>,
        String,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let recorder = Arc::new(Recorder::default());
        let service = service(
            &temp.path().join("config"),
            &temp.path().join("data"),
            recorder.clone(),
        );
        let handle = service.handle_for_user(USER).await.unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let root = handle
            .save_recent_project_root(path_string(&root))
            .await
            .unwrap()
            .project_root;
        (temp, service, handle, recorder, root)
    }

    fn named<'a>(roots: &'a [RecentProjectRoot], path: &str) -> &'a RecentProjectRoot {
        roots.iter().find(|root| root.path == path).unwrap()
    }

    #[tokio::test]
    async fn project_name_is_durable_and_reset_retains_a_newer_revision() {
        let (temp, _, handle, recorder, root) = fixture().await;
        let roots = handle
            .rename_project_root(root.clone(), "  Research  ".into())
            .await
            .unwrap();
        let first = named(&roots, &root);
        assert_eq!(first.name, "Research");
        assert!(first.name_revision > 0);
        assert!(!first.is_default_workspace);
        assert!(
            matches!(&recorder.events()[0], AgentServiceEvent::ProjectsChanged { user_id, roots: event_roots }
            if user_id == USER && event_roots == &roots)
        );
        let restarted = service(
            &temp.path().join("config"),
            &temp.path().join("data"),
            Arc::new(Recorder::default()),
        );
        let restarted = restarted.handle_for_user(USER).await.unwrap();
        assert_eq!(
            named(&restarted.list_recent_project_roots().await.unwrap(), &root),
            first
        );
        let reset = restarted
            .rename_project_root(root.clone(), "project".into())
            .await
            .unwrap();
        let reset = named(&reset, &root);
        assert_eq!(reset.name, "project");
        assert!(reset.name_revision > first.name_revision);
        restarted
            .migrate_project_names(HashMap::from([(root.clone(), "Old alias".into())]))
            .await
            .unwrap();
        assert_eq!(
            named(&restarted.list_recent_project_roots().await.unwrap(), &root),
            reset
        );
        let again = restarted
            .rename_project_root(root.clone(), "project".into())
            .await
            .unwrap();
        assert_eq!(
            named(&again, &root),
            reset,
            "a repeated setter is idempotent"
        );
    }

    #[tokio::test]
    async fn project_names_do_not_roam_or_cross_accounts() {
        let (temp, first_service, handle, _, root) = fixture().await;
        handle
            .rename_project_root(root.clone(), "Device one".into())
            .await
            .unwrap();
        let other_account = first_service
            .handle_for_user("other@example.com")
            .await
            .unwrap();
        assert!(
            other_account
                .rename_project_root(root.clone(), "Denied".into())
                .await
                .is_err()
        );
        other_account
            .save_recent_project_root(root.clone())
            .await
            .unwrap();
        assert_eq!(
            named(
                &other_account.list_recent_project_roots().await.unwrap(),
                &root
            )
            .name,
            "project"
        );
        other_account
            .rename_project_root(root.clone(), "Other account".into())
            .await
            .unwrap();
        assert_eq!(
            named(&handle.list_recent_project_roots().await.unwrap(), &root).name,
            "Device one"
        );

        // The config root is deliberately shared, as with an OS roaming profile.
        let device_two = service(
            &temp.path().join("config"),
            &temp.path().join("device-two"),
            Arc::new(Recorder::default()),
        );
        let device_two = device_two.handle_for_user(USER).await.unwrap();
        assert_eq!(
            named(
                &device_two.list_recent_project_roots().await.unwrap(),
                &root
            )
            .name,
            "project"
        );
        device_two
            .rename_project_root(root.clone(), "Device two".into())
            .await
            .unwrap();
        assert_eq!(
            named(&handle.list_recent_project_roots().await.unwrap(), &root).name,
            "Device one"
        );
        assert!(
            handle
                .project_names_path()
                .unwrap()
                .starts_with(temp.path().join("data"))
        );
        let roaming = std::fs::read_to_string(
            account_config_dir_path(handle.paths(), USER)
                .unwrap()
                .join("recent_roots.json"),
        )
        .unwrap();
        assert!(!roaming.contains("Device one"));
        assert!(!roaming.contains("Device two"));
    }

    #[tokio::test]
    async fn legacy_import_is_owner_only_once_and_keeps_explicit_resets() {
        let (temp, _, handle, _, root) = fixture().await;
        let removed = temp.path().join("removed");
        std::fs::create_dir(&removed).unwrap();
        let removed = handle
            .save_recent_project_root(path_string(&removed))
            .await
            .unwrap()
            .project_root;
        handle
            .remove_project_root(removed.clone(), None)
            .await
            .unwrap();
        let unknown = format!("{root}-not-owned");
        let legacy = HashMap::from([
            (root.clone(), "Legacy owner".into()),
            (removed.clone(), "Legacy removed".into()),
            (unknown.clone(), "Unknown root".into()),
        ]);
        assert!(!handle.project_names_migrated().await.unwrap());
        handle.migrate_project_names(legacy.clone()).await.unwrap();
        assert!(handle.project_names_migrated().await.unwrap());
        assert_eq!(
            named(&handle.list_recent_project_roots().await.unwrap(), &root).name,
            "Legacy owner"
        );
        let stored = ProjectNames::load(&handle.project_names_path().unwrap()).unwrap();
        assert!(stored.legacy_imported);
        assert!(!stored.names.contains_key(&unknown));
        assert_eq!(
            stored.names[&removed].name.as_deref(),
            Some("Legacy removed")
        );
        assert!(
            handle
                .list_recent_project_roots()
                .await
                .unwrap()
                .iter()
                .all(|r| r.path != removed)
        );
        let restored = handle
            .save_recent_project_root(removed.clone())
            .await
            .unwrap();
        assert_eq!(named(&restored.roots, &removed).name, "Legacy removed");
        handle
            .rename_project_root(root.clone(), "project".into())
            .await
            .unwrap();
        handle.migrate_project_names(legacy).await.unwrap();
        assert_eq!(
            named(&handle.list_recent_project_roots().await.unwrap(), &root).name,
            "project"
        );
    }

    #[tokio::test]
    async fn concurrent_project_renames_merge_and_publish_committed_names_in_order() {
        let (temp, _, handle, recorder, root) = fixture().await;
        let second = temp.path().join("second");
        std::fs::create_dir(&second).unwrap();
        let second = handle
            .save_recent_project_root(path_string(&second))
            .await
            .unwrap()
            .project_root;
        let (a, b) = tokio::join!(
            handle.rename_project_root(root.clone(), "First label".into()),
            handle.rename_project_root(second.clone(), "Second label".into())
        );
        a.unwrap();
        b.unwrap();
        let roots = handle.list_recent_project_roots().await.unwrap();
        assert_eq!(named(&roots, &root).name, "First label");
        assert_eq!(named(&roots, &second).name, "Second label");
        let events = recorder.events();
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[1], AgentServiceEvent::ProjectsChanged { roots: event_roots, .. } if event_roots == &roots)
        );
        assert_ne!(
            named(&roots, &root).name_revision,
            named(&roots, &second).name_revision
        );
        // A rename cannot replace settings written by a separate narrow command.
        let mut config = handle.load_config().await.unwrap();
        config.default_model = "fixture-model".into();
        let (saved, renamed) = tokio::join!(
            handle.save_config(config),
            handle.rename_project_root(root.clone(), "Changed".into())
        );
        saved.unwrap();
        renamed.unwrap();
        assert_eq!(
            handle.load_config().await.unwrap().default_model,
            "fixture-model"
        );
        assert_eq!(
            named(&handle.list_recent_project_roots().await.unwrap(), &root).name,
            "Changed"
        );
    }

    #[tokio::test]
    async fn names_survive_register_remove_restore_and_reorder_without_changing_paths() {
        let (_temp, _, handle, _, root) = fixture().await;
        let named_roots = handle
            .rename_project_root(root.clone(), "Persistent label".into())
            .await
            .unwrap();
        let expected = named(&named_roots, &root).clone();
        let config_before = handle.load_config().await.unwrap();
        let selected = handle.save_recent_project_root(root.clone()).await.unwrap();
        assert_eq!(named(&selected.roots, &root), &expected);
        assert_eq!(selected.project_root, root);
        handle
            .remove_project_root(root.clone(), None)
            .await
            .unwrap();
        assert!(
            handle
                .list_recent_project_roots()
                .await
                .unwrap()
                .iter()
                .all(|r| r.path != root)
        );
        assert!(
            handle
                .rename_project_root(root.clone(), "Hidden".into())
                .await
                .is_err()
        );
        let restored = handle.save_recent_project_root(root.clone()).await.unwrap();
        assert_eq!(named(&restored.roots, &root).name, expected.name);
        assert_eq!(
            named(&restored.roots, &root).name_revision,
            expected.name_revision
        );
        let reordered = handle
            .save_project_root_order(vec![root.clone()])
            .await
            .unwrap();
        assert_eq!(named(&reordered, &root).name, expected.name);
        assert_eq!(
            handle.load_config().await.unwrap().project_trust,
            config_before.project_trust
        );
        assert!(Path::new(&root).is_dir());
        assert!(!Path::new(&root).with_file_name("Persistent label").exists());
    }

    #[tokio::test]
    async fn task_only_roots_have_shared_names_and_unknown_roots_are_rejected() {
        let (temp, _, handle, recorder, root) = fixture().await;
        let acp = temp.path().join("acp-only");
        std::fs::create_dir(&acp).unwrap();
        let acp = path_string(&acp.canonicalize().unwrap());
        handle
            .store()
            .unwrap()
            .insert(&TaskRow::new(
                "acp-task".into(),
                "ACP task".into(),
                acp.clone(),
                TaskKind::Acp,
                None,
                1,
            ))
            .unwrap();
        let renamed = handle
            .rename_project_root(acp.clone(), "ACP project".into())
            .await
            .unwrap();
        assert_eq!(named(&renamed, &acp).name, "ACP project");
        let event_count = recorder.events().len();
        for bad_name in [
            "",
            " \t ",
            "two\nlines",
            "x\u{2028}y",
            "\u{200b}",
            &"x".repeat(129),
        ] {
            assert!(
                handle
                    .rename_project_root(root.clone(), bad_name.into())
                    .await
                    .is_err()
            );
        }
        assert!(
            handle
                .rename_project_root(format!("{root}-unknown"), "Valid".into())
                .await
                .is_err()
        );
        assert_eq!(
            recorder.events().len(),
            event_count,
            "rejected mutations do not publish"
        );
        assert!(normalize_name(&"é".repeat(128)).is_ok());
    }

    #[test]
    fn default_workspace_is_owner_resolved_even_when_already_registered() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("Owner Documents").join("Maple");
        let decoy = temp.path().join("Different").join("Maple");
        let path = path_string(&workspace);
        let decoy_path = path_string(&decoy);
        let mut roots = catalog(
            vec![
                project_root_record(path.clone(), 1),
                project_root_record(decoy_path.clone(), 2),
            ],
            [],
            &[],
            Some(&workspace),
        );
        let mut names = ProjectNames::default();
        names.apply(&mut roots, Some(&workspace));
        assert_eq!(named(&roots, &path).name, "Maple Workspace");
        assert!(named(&roots, &path).is_default_workspace);
        assert_eq!(named(&roots, &decoy_path).name, "Maple");
        assert!(!named(&roots, &decoy_path).is_default_workspace);
        names
            .set(path.clone(), Some("Custom workspace".into()))
            .unwrap();
        names.apply(&mut roots, Some(&workspace));
        assert_eq!(named(&roots, &path).name, "Custom workspace");
        assert!(named(&roots, &path).is_default_workspace);
        names.set(path.clone(), None).unwrap();
        names.apply(&mut roots, Some(&workspace));
        assert_eq!(named(&roots, &path).name, "Maple Workspace");
        assert!(named(&roots, &path).name_revision > 0);
        let mut synthetic = catalog(Vec::new(), [], &[], Some(&workspace));
        names.apply(&mut synthetic, Some(&workspace));
        assert_eq!(
            named(&synthetic, &path),
            &RecentProjectRoot {
                last_used_ms: 0,
                ..named(&roots, &path).clone()
            }
        );
        assert!(catalog(Vec::new(), [], &[path], Some(&workspace)).is_empty());
    }

    #[tokio::test]
    async fn invalid_metadata_fails_closed_without_events_or_settings_loss() {
        let (_temp, _, handle, recorder, root) = fixture().await;
        let file = handle.project_names_path().unwrap();
        write_device_local_json_file(&file, &serde_json::json!({"revision": 2, "names": {root.clone(): {"name": "broken\nname", "revision": 2}}})).unwrap();
        assert!(
            handle
                .rename_project_root(root, "Replacement".into())
                .await
                .is_err()
        );
        assert!(handle.migrate_project_names(HashMap::new()).await.is_err());
        assert!(recorder.events().is_empty());
        assert!(std::fs::read_to_string(file).unwrap().contains("broken"));
    }
}
