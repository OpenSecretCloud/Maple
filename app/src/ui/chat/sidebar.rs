//! The task sidebar: the project switcher, the virtualized row list, and
//! the actions that pin, settle, rename, trust, or archive a task.
//! Rows and sections are rebuilt when the session list changes, never on
//! a frame.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, Context, Div, Entity, Focusable, SharedString, Window, div, prelude::*,
    px,
};
use maple_agent::agent::{AgentProjectTrustStatus, AgentSessionSummary};

use super::commands::ChatCommand;
use super::{ChatScreen, MenuAction, RenameTarget, SIDEBAR_WIDTH, SessionActivity, section_label};
use crate::ui::icons::{icon, spinner_with_id, wordmark};
use crate::ui::motion;
use crate::ui::text_input::TextInput;
use crate::ui::theme;
use crate::ui::titlebar;
use crate::ui::widgets;

/// Strings and element ids one sidebar task row shows, built when the
/// session list changes instead of on every frame.
pub(super) struct SidebarRow {
    pub(super) id: Arc<str>,
    pub(super) element_id: SharedString,
    /// Hover group that reveals the row's action buttons.
    pub(super) group: SharedString,
    pub(super) rename_id: SharedString,
    pub(super) archive_id: SharedString,
    /// Animation id of the running indicator.
    pub(super) spinner_id: SharedString,
    pub(super) pin_id: SharedString,
    pub(super) menu_id: SharedString,
    /// Id of the overflow-menu panel itself.
    pub(super) menu_panel_id: SharedString,
    pub(super) menu_rename_id: SharedString,
    pub(super) menu_pin_id: SharedString,
    pub(super) menu_settle_id: SharedString,
    pub(super) menu_archive_id: SharedString,
    pub(super) title: SharedString,
    /// Display name of the task's project, shown on every row.
    pub(super) project_name: SharedString,
    /// Lower-cased title, matched against the sidebar filter.
    pub(super) search: String,
}

impl SidebarRow {
    fn build(session: &AgentSessionSummary, project_name: &str) -> Self {
        let id = &session.id;
        Self {
            id: Arc::from(id.as_str()),
            element_id: SharedString::from(format!("session-{id}")),
            group: SharedString::from(format!("task-row-{id}")),
            rename_id: SharedString::from(format!("rename-session-{id}")),
            archive_id: SharedString::from(format!("archive-session-{id}")),
            spinner_id: SharedString::from(format!("spinner-session-{id}")),
            pin_id: SharedString::from(format!("pin-session-{id}")),
            menu_id: SharedString::from(format!("menu-session-{id}")),
            menu_panel_id: SharedString::from(format!("task-menu-{id}")),
            menu_rename_id: SharedString::from(format!("rename-task-{id}")),
            menu_pin_id: SharedString::from(format!("pin-task-{id}")),
            menu_settle_id: SharedString::from(format!("settle-task-{id}")),
            menu_archive_id: SharedString::from(format!("archive-task-{id}")),
            title: SharedString::from(session.title.clone()),
            project_name: SharedString::from(project_name.to_string()),
            search: session.title.to_lowercase(),
        }
    }
}

/// The popup menu the sidebar shows. The project menu opens inside the
/// switcher menu, so its variant comes first in `sidebar_popup`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SidebarPopup {
    Switcher,
    Project(String),
    Task(String),
}

/// One row of the virtualized sidebar list, in display order. Rebuilt
/// with the sections and when a section folds; the list builds only the
/// rows on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarEntry {
    NewTask,
    /// The project switcher row above the sections.
    ProjectsHeader,
    /// A section label, shown only when the section has rows.
    SectionLabel(SidebarSection),
    Task(SidebarTaskEntry),
    ArchivedHeader,
    /// Placeholder row when no task row is listed.
    Empty(SidebarEmpty),
}

/// A task row: its index into `sessions` plus the section flags the
/// rebuild already decided, so render never re-derives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SidebarTaskEntry {
    pub(super) session: usize,
    pub(super) pinned: bool,
    pub(super) settled: bool,
    pub(super) archived: bool,
}

/// Why the task list has no rows to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarEmpty {
    /// No tasks exist yet.
    NoTasks,
    /// The search filter matched nothing.
    NoMatches,
}

/// A named section of the inbox: pinned tasks, active work, and the
/// settled rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarSection {
    Pinned,
    Active,
    Settled,
}

impl SidebarSection {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Pinned => "PINNED",
            Self::Active => "ACTIVE",
            Self::Settled => "SETTLED",
        }
    }

    pub(super) fn element_id(self) -> &'static str {
        match self {
            Self::Pinned => "section-toggle-pinned",
            Self::Active => "section-toggle-active",
            Self::Settled => "section-toggle-settled",
        }
    }
}

/// One selectable action of a sidebar popup menu. Building the list once
/// keeps the render path and the Application-Vim Enter key on the same
/// items.
struct SidebarMenuItem {
    id: SharedString,
    icon: &'static str,
    label: &'static str,
    on_click: MenuAction,
}

/// One project the switcher menu lists: the root path, its display name,
/// and the element ids its row and menus use. Built when the sections
/// rebuild, never per frame.
pub(super) struct SwitcherRoot {
    root: String,
    name: SharedString,
    row_id: SharedString,
    row_group: SharedString,
    menu_id: SharedString,
}

impl ChatScreen {
    pub(super) fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.sidebar_collapsed == collapsed {
            return;
        }
        self.sidebar_collapsed = collapsed;
        cx.notify();
    }

    pub(super) fn toggle_sidebar_visibility(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_collapsed(!self.sidebar_collapsed, cx);
    }

    pub(super) fn set_archived_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.archived_expanded == expanded {
            return;
        }
        self.archived_expanded = expanded;
        self.rebuild_sidebar_entries();
        cx.notify();
    }

    pub(super) fn toggle_archived_visibility(&mut self, cx: &mut Context<Self>) {
        self.set_archived_expanded(!self.archived_expanded, cx);
    }

    pub(super) fn focus_sidebar_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_collapsed(false, cx);
        if let Some(input) = self.search_input.clone() {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Rebuild the sidebar sections: pinned tasks first, then the active
    /// inbox, then the settled rest. Rows are one flat list
    /// across projects and each row names its project. The order is
    /// derived, never written back, and changes only when activity or the
    /// user changes it. Called when sessions, roots, names, or the filter
    /// change, not per render.
    pub(super) fn rebuild_sidebar_sections(&mut self) {
        self.sync_sidebar_rows();
        let filter = self.sidebar_filter.as_str();
        let filtering = !filter.is_empty();
        let scoped = self.sidebar_project_filter.as_deref();
        let pinned_ids: HashSet<&str> = self.pinned_tasks.iter().map(String::as_str).collect();
        let mut pinned = Vec::new();
        let mut active = Vec::new();
        let mut settled = Vec::new();
        let mut archived_indices = Vec::new();
        // Roots that at least one stored task names, in first-seen order,
        // with a set beside the vec so membership stays a hash lookup.
        let mut session_roots: Vec<&str> = Vec::new();
        let mut seen_roots: HashSet<&str> = HashSet::new();
        let mut root_search: HashMap<&str, String> = HashMap::new();
        for (index, session) in self.sessions.iter().enumerate() {
            let root = session.project_root.as_str();
            if seen_roots.insert(root) {
                session_roots.push(root);
            }
            // A task with no messages is a draft: it joins the inbox
            // only once its first message is sent.
            if session.message_count == 0 {
                continue;
            }
            if scoped.is_some_and(|scoped| scoped != root) {
                continue;
            }
            let matches = !filtering
                || self.sidebar_rows[index].search.contains(filter)
                || root_search
                    .entry(root)
                    .or_insert_with(|| self.root_name(root).to_lowercase())
                    .contains(filter);
            if !matches {
                continue;
            }
            if session.archived {
                archived_indices.push(index);
            } else if pinned_ids.contains(session.id.as_str()) {
                pinned.push(index);
            } else if self.session_is_active(session) {
                active.push(index);
            } else {
                settled.push(index);
            }
        }
        // Every section reads newest activity first.
        let by_update = |ix: &usize| {
            std::cmp::Reverse(
                self.sessions
                    .get(*ix)
                    .map(|session| session.updated_ms)
                    .unwrap_or(0),
            )
        };
        pinned.sort_by_cached_key(by_update);
        // A task woken by hand goes to the top of the active section so
        // the un-settle click visibly moves it.
        active.sort_by_cached_key(|ix| {
            let unsettled = self
                .sessions
                .get(*ix)
                .is_some_and(|session| self.unsettled_tasks.contains(&session.id));
            (std::cmp::Reverse(unsettled), by_update(ix))
        });
        settled.sort_by_cached_key(by_update);
        self.sidebar_pinned = pinned;
        self.sidebar_active = active;
        self.sidebar_settled = settled;
        self.archived_indices = archived_indices;
        self.sidebar_scope_label = self
            .sidebar_project_filter
            .as_deref()
            .map(|root| self.root_name(root))
            .map(SharedString::from)
            .unwrap_or_else(|| "All projects".into());
        // The switcher lists the saved roots, then roots only tasks know
        // about, alphabetically so they never move as sessions change.
        let recent_roots: HashSet<&str> = self.recent_roots.iter().map(String::as_str).collect();
        let mut fresh_roots: Vec<&str> = session_roots
            .iter()
            .copied()
            .filter(|root| !recent_roots.contains(root))
            .collect();
        fresh_roots.sort_unstable();
        self.switcher_roots = self
            .recent_roots
            .iter()
            .map(String::as_str)
            .chain(fresh_roots.iter().copied())
            .filter(|root| seen_roots.contains(root) || recent_roots.contains(root))
            .map(|root| SwitcherRoot {
                name: self.root_name(root).into(),
                root: root.to_string(),
                row_id: SharedString::from(format!("switcher-{root}")),
                row_group: SharedString::from(format!("switcher-project-row-{root}")),
                menu_id: SharedString::from(format!("menu-project-{root}")),
            })
            .collect();
        self.rebuild_sidebar_entries();
    }

    /// Whether a task belongs to the active inbox. Every task stays
    /// active until it is settled away by hand; a run in flight is
    /// activity, so it wakes even a settled task.
    fn session_is_active(&self, session: &AgentSessionSummary) -> bool {
        self.active_runs.contains_key(&session.id) || !self.settled_tasks.contains(&session.id)
    }

    /// Flatten the sections into list rows. The list splices only the
    /// span that changed, so the scroll position and the measured
    /// heights of untouched rows survive every rebuild.
    pub(super) fn rebuild_sidebar_entries(&mut self) {
        let mut entries = vec![SidebarEntry::NewTask, SidebarEntry::ProjectsHeader];
        let tasks = |entries: &mut Vec<SidebarEntry>,
                     section: SidebarSection,
                     rows: &[usize],
                     expanded: bool| {
            if rows.is_empty() {
                return;
            }
            entries.push(SidebarEntry::SectionLabel(section));
            if expanded {
                entries.extend(rows.iter().map(|&session| {
                    SidebarEntry::Task(SidebarTaskEntry {
                        session,
                        pinned: section == SidebarSection::Pinned,
                        settled: section == SidebarSection::Settled,
                        archived: false,
                    })
                }));
            }
        };
        tasks(
            &mut entries,
            SidebarSection::Pinned,
            &self.sidebar_pinned,
            self.section_expanded(SidebarSection::Pinned),
        );
        tasks(
            &mut entries,
            SidebarSection::Active,
            &self.sidebar_active,
            self.section_expanded(SidebarSection::Active),
        );
        tasks(
            &mut entries,
            SidebarSection::Settled,
            &self.sidebar_settled,
            self.section_expanded(SidebarSection::Settled),
        );
        if !self.archived_indices.is_empty() {
            entries.push(SidebarEntry::ArchivedHeader);
            if self.archived_expanded {
                entries.extend(self.archived_indices.iter().map(|&session| {
                    SidebarEntry::Task(SidebarTaskEntry {
                        session,
                        archived: true,
                        pinned: false,
                        settled: false,
                    })
                }));
            }
        }
        if !entries
            .iter()
            .any(|entry| matches!(entry, SidebarEntry::Task(_) | SidebarEntry::ArchivedHeader))
        {
            entries.push(SidebarEntry::Empty(if self.sidebar_filter.is_empty() {
                SidebarEmpty::NoTasks
            } else {
                SidebarEmpty::NoMatches
            }));
        }
        let old = std::mem::replace(&mut self.sidebar_entries, entries);
        if self.application_vim_enabled {
            self.rebuild_sidebar_application_targets();
        }
        // Trim the common prefix and suffix so only rows that really
        // changed are re-measured. gpui resets the scroll top to the
        // start of the spliced range, so a whole-list splice would jump
        // back to the top of the sidebar.
        let common = old.len().min(self.sidebar_entries.len());
        let mut prefix = 0;
        while prefix < common && old[prefix] == self.sidebar_entries[prefix] {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < common - prefix
            && old[old.len() - 1 - suffix]
                == self.sidebar_entries[self.sidebar_entries.len() - 1 - suffix]
        {
            suffix += 1;
        }
        if prefix + suffix < old.len() || prefix + suffix < self.sidebar_entries.len() {
            self.sidebar_list.splice(
                prefix..old.len() - suffix,
                self.sidebar_entries.len() - prefix - suffix,
            );
        }
        if self.application_vim_enabled {
            self.reconcile_sidebar_application_selection();
        }
    }

    /// A row changed its height (a rename field came or went): let the
    /// list measure that one row again. The rest of the list keeps its
    /// scroll and its cached heights.
    pub(super) fn remeasure_sidebar(&self, target: &RenameTarget) {
        let row = match target {
            RenameTarget::Task(id) => self.sidebar_entries.iter().position(|entry| {
                matches!(entry, SidebarEntry::Task(task)
                    if self
                        .sessions
                        .get(task.session)
                        .is_some_and(|session| session.id == *id))
            }),
            // The project rename field lives in the switcher menu, which
            // hangs off the projects header row.
            RenameTarget::Project(_) => self
                .sidebar_entries
                .iter()
                .position(|entry| matches!(entry, SidebarEntry::ProjectsHeader)),
        };
        if let Some(row) = row {
            self.sidebar_list.remeasure_items(row..row + 1);
        }
    }

    /// Reveal the switcher root paths for tests.
    #[cfg(test)]
    pub(super) fn switcher_root_paths(&self) -> Vec<&str> {
        self.switcher_roots
            .iter()
            .map(|root| root.root.as_str())
            .collect()
    }

    /// Rebuild the per-session sidebar strings when the session list
    /// moved under them; a plain filter change reuses them.
    pub(super) fn sync_sidebar_rows(&mut self) {
        // Compare against the stored name without building one: this
        // runs on every rebuild and a fresh list allocates nothing.
        let fresh = self.sidebar_rows.len() == self.sessions.len()
            && self
                .sidebar_rows
                .iter()
                .zip(&self.sessions)
                .all(|(row, session)| {
                    *row.id == *session.id
                        && row.title.as_ref() == session.title
                        && self.root_name_matches(&session.project_root, &row.project_name)
                });
        if fresh {
            return;
        }
        self.sidebar_rows = self
            .sessions
            .iter()
            .map(|session| SidebarRow::build(session, &self.root_name(&session.project_root)))
            .collect();
        // A rename or a reordered list moves the selected task's title.
        self.refresh_selected_title();
    }

    pub(super) fn search_changed(&mut self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let filter = input.read(cx).text_ref().trim().to_lowercase();
        if filter != self.sidebar_filter {
            self.sidebar_filter = filter;
            self.rebuild_sidebar_sections();
            cx.notify();
        }
    }

    pub(super) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |input, cx| input.clear(cx));
        }
        self.sidebar_filter.clear();
        self.rebuild_sidebar_sections();
        cx.notify();
    }

    /// Apply `update` to the settings file off the UI thread.
    fn persist_settings(
        &self,
        update: impl FnOnce(&mut crate::settings::AppSettings) + Send + 'static,
        _cx: &mut Context<Self>,
    ) {
        crate::settings::update_settings_in_background(update);
    }

    /// Pin or unpin a task. Pinned tasks stay at the top of the sidebar
    /// and persist in the app settings.
    pub(super) fn toggle_task_pin(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if self.pinned_tasks.iter().any(|pinned| pinned == session_id) {
            self.pinned_tasks.retain(|pinned| pinned != session_id);
        } else {
            self.pinned_tasks.push(session_id.to_string());
        }
        self.rebuild_sidebar_sections();
        cx.notify();
        let pinned = self.pinned_tasks.clone();
        self.persist_settings(move |settings| settings.pinned_tasks = pinned, cx);
    }

    /// Move a task to the settled section: it leaves the active inbox
    /// until new activity wakes it again.
    pub(super) fn settle_task(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.unsettled_tasks.remove(session_id);
        if self.settled_tasks.insert(session_id.to_string()) {
            self.rebuild_sidebar_sections();
            cx.notify();
            Self::persist_task_sets(self.settled_tasks.clone(), self.unsettled_tasks.clone(), cx);
        }
    }

    /// Whether a collapsible section shows its rows.
    fn section_expanded(&self, section: SidebarSection) -> bool {
        match section {
            SidebarSection::Pinned => self.pinned_expanded,
            SidebarSection::Active => self.active_expanded,
            SidebarSection::Settled => self.settled_expanded,
        }
    }

    fn toggle_section_expanded(&mut self, section: SidebarSection, cx: &mut Context<Self>) {
        match section {
            SidebarSection::Pinned => self.pinned_expanded = !self.pinned_expanded,
            SidebarSection::Active => self.active_expanded = !self.active_expanded,
            SidebarSection::Settled => self.settled_expanded = !self.settled_expanded,
        }
        self.rebuild_sidebar_entries();
        cx.notify();
    }

    /// Move a task back into the active inbox at the top. It stays
    /// active until it is settled away by hand again.
    pub(super) fn unsettle_task(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.settled_tasks.remove(session_id);
        if self.unsettled_tasks.insert(session_id.to_string()) {
            self.rebuild_sidebar_sections();
            cx.notify();
            Self::persist_task_sets(self.settled_tasks.clone(), self.unsettled_tasks.clone(), cx);
        }
    }

    /// Write the settle/unsettle sets in one background update.
    fn persist_task_sets(
        settled: std::collections::HashSet<String>,
        unsettled: std::collections::HashSet<String>,
        _cx: &mut Context<Self>,
    ) {
        crate::settings::update_settings_in_background(move |settings| {
            settings.settled_tasks = settled.into_iter().collect();
            settings.unsettled_tasks = unsettled.into_iter().collect();
        });
    }

    /// Open or close the project switcher menu.
    pub(super) fn toggle_switcher_menu(&mut self, cx: &mut Context<Self>) {
        self.project_menu = None;
        self.set_switcher_menu_open(!self.switcher_menu_open, cx);
    }

    /// Deterministically open or close the project switcher menu.
    /// Application Vim uses this setter; pointer clicks toggle above.
    pub(super) fn set_switcher_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.sidebar_menu_selected = None;
        if self.switcher_menu_open == open {
            return;
        }
        self.switcher_menu_open = open;
        if open {
            self.project_menu = None;
        }
        cx.notify();
    }

    /// Scope the sidebar to one project, or show every project again.
    pub(super) fn set_sidebar_project_filter(
        &mut self,
        root: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_menu_selected = None;
        self.sidebar_project_filter = root.filter(|root| !root.is_empty());
        self.switcher_menu_open = false;
        self.rebuild_sidebar_sections();
        cx.notify();
    }

    /// Display name for a project root: the saved name, else the folder name.
    pub(super) fn root_name(&self, root: &str) -> String {
        self.project_names
            .get(root)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| root_display_name(root))
    }

    /// Whether `name` is the display name of `root`, without building the
    /// name the way `root_name` does.
    fn root_name_matches(&self, root: &str, name: &str) -> bool {
        match self
            .project_names
            .get(root)
            .filter(|name| !name.trim().is_empty())
        {
            Some(stored) => stored == name,
            None => match std::path::Path::new(root).file_name() {
                // Invalid UTF-8 misses here and falls to a rebuild, which
                // stays correct.
                Some(file) => file.to_string_lossy().as_ref() == name,
                None => root == name,
            },
        }
    }

    /// Start an inline rename of a task or project in the sidebar.
    pub(super) fn begin_rename(&mut self, target: RenameTarget, cx: &mut Context<Self>) {
        let current = match &target {
            RenameTarget::Task(id) => self
                .sessions
                .iter()
                .find(|session| &session.id == id)
                .map(|session| session.title.clone())
                .unwrap_or_default(),
            RenameTarget::Project(root) => self.root_name(root),
        };
        let chat = cx.entity().downgrade();
        let rename_chat = chat.clone();
        let application_vim_enabled = self.application_vim_enabled;
        let input = cx.new(move |cx| {
            let mut input = TextInput::new("Name", cx)
                .with_tab_index(0)
                .application_vim(application_vim_enabled);
            input.set_text(&current, cx);
            input
                .on_application_escape(move |window, cx| {
                    if let Some(chat) = rename_chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.focus_application_vim(window, cx));
                    }
                })
                .on_enter(move |_text, _, cx| {
                    let chat = chat.clone();
                    // commit_rename reads this input; defer out of its update.
                    cx.defer(move |cx| {
                        if let Some(chat) = chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.commit_rename(cx));
                        }
                    });
                })
        });
        self.project_menu = None;
        self.rename = Some(target);
        self.rename_input = Some(input);
        self.rename_focus_pending = true;
        if let Some(target) = self.rename.as_ref() {
            self.remeasure_sidebar(target);
        }
        cx.notify();
    }

    pub(super) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = self.rename.as_ref() {
            self.remeasure_sidebar(target);
        }
        self.rename = None;
        self.rename_input = None;
        cx.notify();
    }

    pub(super) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.rename.take() else {
            return;
        };
        let name = self
            .rename_input
            .take()
            .map(|input| input.read(cx).text())
            .unwrap_or_default();
        let name = name.trim().to_string();
        self.remeasure_sidebar(&target);
        cx.notify();
        if name.is_empty() {
            return;
        }
        match target {
            RenameTarget::Task(session_id) => {
                let backend = self.backend.clone();
                let user_id = self.user_id.clone();
                self.call(
                    async move { backend.rename_session(&user_id, &session_id, name).await },
                    cx,
                    |this, result, cx| {
                        match result {
                            Ok(session) => {
                                this.upsert_session(session);
                                this.rebuild_sidebar_sections();
                            }
                            Err(message) => this.notice = Some(message.into()),
                        }
                        cx.notify();
                    },
                );
            }
            RenameTarget::Project(root) => {
                if name == root_display_name(&root) {
                    self.project_names.remove(&root);
                } else {
                    self.project_names.insert(root.clone(), name);
                }
                self.rebuild_sidebar_sections();
                let names = self.project_names.clone();
                self.persist_settings(move |settings| settings.project_names = names, cx);
            }
        }
    }

    /// Show the project's folder in the file manager.
    fn open_folder(&mut self, root: &str, cx: &mut Context<Self>) {
        self.project_menu = None;
        let path = root.to_string();
        cx.notify();
        self.call(
            async move {
                tokio::task::spawn_blocking(move || crate::platform::reveal_folder(&path))
                    .await
                    .map_err(|error| format!("Could not open the folder: {error}"))?
            },
            cx,
            |this, result, cx| {
                if let Err(message) = result {
                    this.notice = Some(message.into());
                    cx.notify();
                }
            },
        );
    }

    fn toggle_project_menu(&mut self, root: &str, cx: &mut Context<Self>) {
        self.task_menu = None;
        self.sidebar_menu_selected = None;
        if self.project_menu.as_deref() == Some(root) {
            self.project_menu = None;
        } else {
            self.project_menu = Some(root.to_string());
            let backend = self.backend.clone();
            let user_id = self.user_id.clone();
            let path = root.to_string();
            self.call(
                async move { backend.project_trust(&user_id, path).await },
                cx,
                |this, result, cx| {
                    if let Ok(status) = result
                        && this.project_menu.as_deref() == Some(status.path.as_str())
                    {
                        this.menu_trust = Some(status);
                        cx.notify();
                    }
                },
            );
        }
        cx.notify();
    }

    /// Open or close the overflow menu of one task row.
    fn toggle_task_menu(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sidebar_menu_selected = None;
        if self.task_menu.as_deref() == Some(session_id) {
            self.task_menu = None;
        } else {
            self.task_menu = Some(session_id.to_string());
        }
        cx.notify();
    }

    /// The overflow menu of a task row: rename, pin, settle, archive.
    fn task_menu_items(&self, task: SidebarTaskEntry) -> Vec<SidebarMenuItem> {
        let Some(row) = self.sidebar_rows.get(task.session) else {
            return Vec::new();
        };
        let rename_id = row.id.to_string();
        let pinned_id = row.id.to_string();
        let settle_id = row.id.to_string();
        let archive_id = row.id.to_string();
        let pinned = task.pinned;
        let archived = task.archived;
        let active = !archived && (self.active_runs.contains_key(&*row.id) || !task.settled);
        vec![
            SidebarMenuItem {
                id: row.menu_rename_id.clone(),
                icon: "pencil",
                label: "Rename task",
                on_click: Box::new(move |this, cx| {
                    this.begin_rename(RenameTarget::Task(rename_id.clone()), cx)
                }),
            },
            SidebarMenuItem {
                id: row.menu_pin_id.clone(),
                icon: "pin",
                label: if pinned { "Unpin task" } else { "Pin task" },
                on_click: Box::new(move |this, cx| this.toggle_task_pin(&pinned_id, cx)),
            },
            SidebarMenuItem {
                id: row.menu_settle_id.clone(),
                icon: if active { "check" } else { "undo-2" },
                label: if active {
                    "Settle task"
                } else {
                    "Unsettle task"
                },
                on_click: Box::new(move |this, cx| {
                    if active {
                        this.settle_task(&settle_id, cx)
                    } else {
                        this.unsettle_task(&settle_id, cx)
                    }
                }),
            },
            SidebarMenuItem {
                id: row.menu_archive_id.clone(),
                icon: if archived {
                    "archive-restore"
                } else {
                    "archive"
                },
                label: if archived {
                    "Restore task"
                } else {
                    "Archive task"
                },
                on_click: Box::new(move |this, cx| {
                    this.set_session_archived(&archive_id, !archived, cx)
                }),
            },
        ]
    }

    /// The overflow menu of one switcher project row.
    fn project_menu_items(&self, root: &str) -> Vec<SidebarMenuItem> {
        let rename_root = root.to_string();
        let open_root = root.to_string();
        let remove_root = root.to_string();
        let mut items = vec![
            SidebarMenuItem {
                id: SharedString::from(format!("rename-project-{root}")),
                icon: "pencil",
                label: "Rename project",
                on_click: Box::new(move |this, cx| {
                    this.begin_rename(RenameTarget::Project(rename_root.clone()), cx)
                }),
            },
            SidebarMenuItem {
                id: SharedString::from(format!("open-project-{root}")),
                icon: "folder-open",
                label: "Open folder",
                on_click: Box::new(move |this, cx| this.open_folder(&open_root, cx)),
            },
        ];
        if let Some(status) = self
            .menu_trust
            .as_ref()
            .filter(|status| status.available && status.path == root)
        {
            let trusted = status.decision == Some(true);
            let path = status.path.clone();
            items.push(SidebarMenuItem {
                id: SharedString::from(format!("trust-project-{root}")),
                icon: if trusted { "shield-check" } else { "lock" },
                label: if trusted {
                    "Untrust project"
                } else {
                    "Trust project"
                },
                on_click: Box::new(move |this, cx| {
                    this.set_project_trust(path.clone(), !trusted, cx)
                }),
            });
        }
        items.push(SidebarMenuItem {
            id: SharedString::from(format!("remove-project-{root}")),
            icon: "trash-2",
            label: "Remove project",
            on_click: Box::new(move |this, cx| this.request_remove_root(&remove_root, cx)),
        });
        items
    }

    /// The popup menu the sidebar shows, if any. The project menu opens
    /// inside the switcher, so it takes priority; the task menu and the
    /// switcher never share the screen with each other.
    fn sidebar_popup(&self) -> Option<SidebarPopup> {
        if let Some(root) = &self.project_menu {
            return Some(SidebarPopup::Project(root.clone()));
        }
        if let Some(session) = &self.task_menu {
            return Some(SidebarPopup::Task(session.clone()));
        }
        if self.switcher_menu_open {
            return Some(SidebarPopup::Switcher);
        }
        None
    }

    /// The task entry whose overflow menu is open.
    fn task_menu_entry(&self, session_id: &str) -> Option<SidebarTaskEntry> {
        self.sidebar_entries.iter().find_map(|entry| match entry {
            SidebarEntry::Task(task) => self
                .sessions
                .get(task.session)
                .filter(|session| session.id == session_id)
                .map(|_| *task),
            _ => None,
        })
    }

    fn sidebar_popup_rows(&self) -> Option<usize> {
        match self.sidebar_popup() {
            Some(SidebarPopup::Switcher) => Some(self.switcher_roots.len() + 2),
            Some(SidebarPopup::Project(root)) => Some(self.project_menu_items(&root).len()),
            Some(SidebarPopup::Task(session)) => Some(
                self.task_menu_entry(&session)
                    .map(|task| self.task_menu_items(task).len())
                    .unwrap_or(0),
            ),
            None => None,
        }
    }

    /// The Application-Vim row highlight of whichever popup menu is open.
    fn sidebar_menu_selection(&self) -> Option<usize> {
        (self.application_vim_enabled && self.sidebar_popup().is_some())
            .then_some(self.sidebar_menu_selected)
            .flatten()
    }

    /// Move the Application-Vim selection inside the open popup menu.
    /// Reports whether one was open.
    pub(super) fn step_sidebar_popup(
        &mut self,
        delta: isize,
        count: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rows) = self.sidebar_popup_rows().filter(|rows| *rows > 0) else {
            return false;
        };
        for _ in 0..count {
            let next = match self.sidebar_menu_selected {
                Some(current) => (current as isize + delta).rem_euclid(rows as isize),
                // Nothing highlighted: enter the menu from the end the key
                // comes from.
                None if delta < 0 => rows as isize - 1,
                None => 0,
            };
            self.sidebar_menu_selected = Some(next as usize);
        }
        cx.notify();
        true
    }

    /// `gg` and `G` inside the open popup menu. Reports whether one was
    /// open.
    pub(super) fn sidebar_popup_edge(&mut self, first: bool, cx: &mut Context<Self>) -> bool {
        let Some(rows) = self.sidebar_popup_rows().filter(|rows| *rows > 0) else {
            return false;
        };
        self.sidebar_menu_selected = Some(if first { 0 } else { rows - 1 });
        cx.notify();
        true
    }

    /// Enter inside the open popup menu: run the highlighted item's
    /// action. Reports whether one was open.
    pub(super) fn activate_sidebar_popup(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(popup) = self.sidebar_popup() else {
            return false;
        };
        let Some(rows) = self.sidebar_popup_rows().filter(|rows| *rows > 0) else {
            return false;
        };
        // Like the project chooser: Enter before any step does nothing.
        let index = match self.sidebar_menu_selected {
            Some(index) => index.min(rows - 1),
            None => return true,
        };
        match popup {
            SidebarPopup::Switcher => {
                if index == 0 {
                    self.set_sidebar_project_filter(None, cx);
                } else if let Some(root) = self.switcher_roots.get(index - 1) {
                    let root = root.root.clone();
                    self.set_sidebar_project_filter(Some(root), cx);
                } else if index == self.switcher_roots.len() + 1 {
                    self.set_switcher_menu_open(false, cx);
                    self.choose_root_dialog(cx);
                }
            }
            SidebarPopup::Project(root) => {
                if let Some(item) = self.project_menu_items(&root).into_iter().nth(index) {
                    let SidebarMenuItem { on_click, .. } = item;
                    on_click(self, cx);
                }
            }
            SidebarPopup::Task(session) => {
                if let Some(task) = self.task_menu_entry(&session)
                    && let Some(item) = self.task_menu_items(task).into_iter().nth(index)
                {
                    let SidebarMenuItem { on_click, .. } = item;
                    on_click(self, cx);
                }
            }
        }
        true
    }

    /// Overflow menu for a task row: pin, settle, and archive, beside the
    /// rename the pencil offers.
    fn render_task_menu(&self, task: SidebarTaskEntry, cx: &mut Context<Self>) -> gpui::Deferred {
        let Some(row) = self.sidebar_rows.get(task.session) else {
            return gpui::deferred(div());
        };
        let selected = self.sidebar_menu_selection();
        let mut menu = div()
            .id(row.menu_panel_id.clone())
            .absolute()
            .top(px(30.))
            .right_0()
            .w(px(180.))
            .py_1()
            .rounded(theme::RADIUS_SM)
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .shadow_md()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.task_menu = None;
                cx.notify();
            }));
        for (index, item) in self.task_menu_items(task).into_iter().enumerate() {
            menu = menu.child(popup_menu_row(item, selected == Some(index), cx));
        }
        gpui::deferred(motion::fade_in(menu, "sidebar-menu-reveal"))
    }

    /// Ask for a trust decision when the current project provides skills
    /// or guidance and none is saved yet.
    pub(super) fn check_project_trust(&mut self, cx: &mut Context<Self>) {
        self.trust_prompt = None;
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.project_trust(&user_id, root).await },
            cx,
            |this, result, cx| {
                if let Ok(status) = result
                    && this.trust_prompts
                    && this.project_root.as_deref() == Some(status.path.as_str())
                    && status.available
                    && !status.protected_features.is_empty()
                    && status.decision.is_none()
                {
                    this.trust_prompt = Some(status);
                    this.dialog_focus.get_or_insert_with(|| cx.focus_handle());
                    this.dialog_focus_pending = true;
                    cx.notify();
                }
            },
        );
    }

    pub(super) fn set_project_trust(
        &mut self,
        path: String,
        trusted: bool,
        cx: &mut Context<Self>,
    ) {
        if self.trust_saving {
            return;
        }
        self.trust_saving = true;
        self.project_menu = None;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.set_project_trust(&user_id, path, trusted).await },
            cx,
            move |this, result, cx| {
                this.trust_saving = false;
                match result {
                    Ok(status) => {
                        if this.trust_prompt.as_ref().map(|p| &p.path) == Some(&status.path) {
                            this.trust_prompt = None;
                        }
                        this.notice = Some(
                            if trusted {
                                "Project trusted: its skills and guidance are available to new tasks"
                            } else {
                                "Project kept untrusted"
                            }
                            .into(),
                        );
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    /// Modal that asks whether project-provided guidance may be used.
    pub(super) fn render_trust_prompt(
        &self,
        status: &AgentProjectTrustStatus,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let name = self.root_name(&status.path);
        let saving = self.trust_saving;
        let button = |id: &'static str, label: &'static str, primary: bool| {
            if primary {
                widgets::primary_button(id)
            } else {
                widgets::secondary_button(id)
            }
            .py_1p5()
            .when(saving, |button| button.opacity(0.6))
            .child(label)
        };
        let keep_path = status.path.clone();
        let trust_path = status.path.clone();
        let key_keep = status.path.clone();
        let key_trust = status.path.clone();
        div()
            .id("trust-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .occlude()
            .bg(theme::scrim())
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("trust-card")
                    .role(gpui::Role::Dialog)
                    .aria_label(format!("Trust {name}?"))
                    .when_some(self.dialog_focus.clone(), |card, focus| {
                        card.track_focus(&focus)
                    })
                    .key_context("Dialog")
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, _window, cx| {
                            if this.trust_saving {
                                return;
                            }
                            match dialog_key(event) {
                                Some(DialogKey::Confirm) => {
                                    this.set_project_trust(key_trust.clone(), true, cx);
                                    cx.stop_propagation();
                                }
                                Some(DialogKey::Cancel) => {
                                    this.set_project_trust(key_keep.clone(), false, cx);
                                    cx.stop_propagation();
                                }
                                None => {}
                            }
                        },
                    ))
                    .w(px(460.))
                    .p_5()
                    .rounded(theme::RADIUS_XL)
                    .shadow_lg()
                    .bg(gpui::rgb(theme::bg_elevated()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child(format!("Trust {name}?")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_secondary()))
                            .child(
                                "Trusting a project lets Maple use project-provided guidance, \
                                 including agent skills. These instructions can influence how \
                                 agents work and use tools. Maple's tool permissions still \
                                 apply, and you can change this choice later from the \
                                 project's menu.",
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(crate::assets::FONT_MONO)
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(status.path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("trust-keep", "Keep untrusted", false).on_click(
                                cx.listener(move |this, _event, _window, cx| {
                                    this.set_project_trust(keep_path.clone(), false, cx);
                                }),
                            ))
                            .child(button("trust-allow", "Trust project", true).on_click(
                                cx.listener(move |this, _event, _window, cx| {
                                    this.set_project_trust(trust_path.clone(), true, cx);
                                }),
                            )),
                    ),
            )
    }

    /// Ask before a project leaves the sidebar.
    fn request_remove_root(&mut self, root: &str, cx: &mut Context<Self>) {
        self.project_menu = None;
        self.confirm_remove_root = Some(root.to_string());
        self.dialog_focus.get_or_insert_with(|| cx.focus_handle());
        self.dialog_focus_pending = true;
        cx.notify();
    }

    fn confirm_remove_root(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.confirm_remove_root.take() {
            self.archive_root(&root, cx);
        }
        cx.notify();
    }

    /// The rename field for the task being edited, if it is this one.
    /// Renders per visible row per frame, so it compares borrowed ids
    /// instead of building a `RenameTarget` to match against.
    fn task_rename_field(&self, session_id: &str) -> Option<gpui::Stateful<Div>> {
        match self.rename.as_ref() {
            Some(RenameTarget::Task(target)) if target == session_id => self.rename_field(),
            _ => None,
        }
    }

    /// The rename field for the project being edited, if it is this one.
    fn project_rename_field(&self, root: &str) -> Option<gpui::Stateful<Div>> {
        match self.rename.as_ref() {
            Some(RenameTarget::Project(target)) if target == root => self.rename_field(),
            _ => None,
        }
    }

    /// The shared rename input, wrapped for a row.
    fn rename_field(&self) -> Option<gpui::Stateful<Div>> {
        let input = self.rename_input.clone()?;
        Some(
            div()
                .id("rename-field")
                .flex_1()
                .min_w_0()
                .on_click(|_event, _window, cx| cx.stop_propagation())
                .child(input),
        )
    }

    /// Overflow menu for a project row.
    fn render_project_menu(&self, root: &str, cx: &mut Context<Self>) -> gpui::Deferred {
        let selected = self.sidebar_menu_selection();
        let mut menu = div()
            .id(SharedString::from(format!("project-menu-{root}")))
            .absolute()
            .top(px(30.))
            .right_0()
            .w(px(180.))
            .py_1()
            .rounded(theme::RADIUS_SM)
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .shadow_md()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.project_menu = None;
                cx.notify();
            }));
        for (index, item) in self.project_menu_items(root).into_iter().enumerate() {
            menu = menu.child(popup_menu_row(item, selected == Some(index), cx));
        }
        gpui::deferred(motion::fade_in(menu, "sidebar-menu-reveal"))
    }

    /// Modal that confirms a project removal.
    pub(super) fn render_confirm_remove(
        &self,
        root: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let name = self.root_name(root);
        let button = |id: &'static str, label: &'static str, primary: bool| {
            if primary {
                widgets::danger_button(id)
            } else {
                widgets::secondary_button(id)
            }
            .py_1p5()
            .child(label)
        };
        div()
            .id("confirm-remove-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .occlude()
            .bg(theme::scrim())
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.confirm_remove_root = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("confirm-remove-card")
                    .role(gpui::Role::Dialog)
                    .aria_label(format!("Remove {name}?"))
                    .when_some(self.dialog_focus.clone(), |card, focus| {
                        card.track_focus(&focus)
                    })
                    .key_context("Dialog")
                    .on_key_down(
                        cx.listener(
                            |this, event: &gpui::KeyDownEvent, _window, cx| match dialog_key(event)
                            {
                                Some(DialogKey::Confirm) => {
                                    this.confirm_remove_root(cx);
                                    cx.stop_propagation();
                                }
                                Some(DialogKey::Cancel) => {
                                    this.confirm_remove_root = None;
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                                None => {}
                            },
                        ),
                    )
                    .w(px(420.))
                    .p_5()
                    .rounded(theme::RADIUS_XL)
                    .shadow_lg()
                    .bg(gpui::rgb(theme::bg_elevated()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_click(|_event, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child(format!("Remove {name}?")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_secondary()))
                            .child(
                                "The project leaves the sidebar and its tasks move to \
                                 Archived, where you can restore them. No files are deleted.",
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(crate::assets::FONT_MONO)
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(root.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("confirm-remove-cancel", "Cancel", false).on_click(
                                cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    this.confirm_remove_root = None;
                                    cx.notify();
                                }),
                            ))
                            .child(button("confirm-remove-ok", "Remove", true).on_click(
                                cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    this.confirm_remove_root(cx);
                                }),
                            )),
                    ),
            )
    }

    /// Drop the selection and everything the composer shows for it: the
    /// transcript, the side thread, the queue, and a permission card that
    /// belongs to the task. Questions stay queued per session.
    pub(super) fn leave_selected_session(&mut self, cx: &mut Context<Self>) {
        let left = self.clear_selected_session_presentation(cx);
        if let Some(left) = left.as_deref() {
            let showing = self
                .pending_permissions
                .iter()
                .any(|permission| permission.session_id == left);
            self.pending_permissions
                .retain(|permission| permission.session_id != left);
            if showing {
                self.permission_responding = false;
            }
        }
    }

    /// Archive or restore one task. The service event updates the row;
    /// an archived selection moves to the newest task in the same root.
    pub(super) fn set_session_archived(
        &mut self,
        session_id: &str,
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        let changed_id = session_id.clone();
        self.call(
            async move {
                backend
                    .set_session_archived(&user_id, &session_id, archived)
                    .await
            },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(session) => {
                        let root = session.project_root.clone();
                        this.upsert_session(session);
                        if archived && this.selected_session.as_deref() == Some(&*changed_id) {
                            this.leave_selected_session(cx);
                            let next = this
                                .sessions
                                .iter()
                                .find(|s| !s.archived && s.project_root == root)
                                .map(|s| s.id.clone());
                            if let Some(id) = next {
                                this.select_session(&id, cx);
                            }
                        }
                    }
                    Err(error) => this.notice = Some(error.into()),
                }
                cx.notify();
            },
        );
    }

    /// Archive every task in a project and drop the project from the
    /// sidebar. The UI selects the next project when this one was current.
    pub(super) fn archive_root(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.root_selecting {
            self.notice = Some("Wait for the project selection to finish, then try again".into());
            cx.notify();
            return;
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let path = root.to_string();
        // Pick the fallback from every known project, not only from the
        // sidebar groups, which a search filter may have narrowed.
        let fallback = self
            .recent_roots
            .iter()
            .chain(
                self.sessions
                    .iter()
                    .filter(|s| !s.archived)
                    .map(|s| &s.project_root),
            )
            .find(|candidate| candidate.as_str() != root)
            .cloned();
        let task_ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| !s.archived && s.project_root == root)
            .map(|s| s.id.clone())
            .collect();
        let removed = path.clone();
        let next_root = fallback.clone();
        let removed_task_ids = task_ids.clone();
        self.call(
            async move {
                for id in task_ids {
                    backend.set_session_archived(&user_id, &id, true).await?;
                }
                backend.remove_project_root(&user_id, path, fallback).await
            },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(()) => {
                        this.recent_roots.retain(|candidate| candidate != &removed);
                        this.pinned_tasks
                            .retain(|candidate| removed_task_ids.iter().all(|id| id != candidate));
                        this.settled_tasks
                            .retain(|candidate| removed_task_ids.iter().all(|id| id != candidate));
                        this.unsettled_tasks
                            .retain(|candidate| removed_task_ids.iter().all(|id| id != candidate));
                        for session in &mut this.sessions {
                            if session.project_root == removed {
                                session.archived = true;
                                this.completed_unread_sessions.remove(&session.id);
                            }
                        }
                        let was_current = this.project_root.as_deref() == Some(&*removed);
                        if was_current {
                            this.leave_selected_session(cx);
                            this.set_project_context(next_root.clone(), cx);
                        }
                        this.rebuild_sidebar_sections();
                        if was_current && let Some(next) = next_root {
                            // The service keeps the fallback out of roaming
                            // config; persist it as the default here, then
                            // open its latest task.
                            this.persist_project_root(next, cx);
                            this.refresh_sessions(cx);
                        } else {
                            this.refresh_roots(cx);
                        }
                    }
                    Err(error) => this.notice = Some(error.into()),
                }
                cx.notify();
            },
        );
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let entity = cx.entity().downgrade();
        // Only the rows on screen (plus a small overdraw) are built each
        // frame; the list keeps the heights of the rest.
        let list = gpui::list(self.sidebar_list.clone(), move |ix, _window, cx| {
            let Some(chat) = entity.upgrade() else {
                return div().into_any_element();
            };
            chat.update(cx, |chat, cx| chat.render_sidebar_entry(ix, cx))
        })
        .size_full();
        div()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::bg_sidebar()))
            .child(titlebar::drag_region(
                div()
                    .id("sidebar-top-row")
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl_4()
                    .pr_3()
                    // The wordmark row sits under the traffic lights, not
                    // beside them; the space above it is still the bar.
                    .pt(titlebar::top_row_top(px(12.)))
                    .pb_2()
                    .child(wordmark(px(16.), theme::text_primary()))
                    .child(self.render_sidebar_toggle(cx)),
            ))
            .children(self.search_input.clone().map(|input| {
                let active = !self.sidebar_filter.is_empty();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mx_4()
                    .mt_3()
                    .px_2()
                    .py_1()
                    .rounded(theme::RADIUS_SM)
                    .bg(gpui::rgb(theme::bg_sidebar_pill()))
                    .border_1()
                    .border_color(gpui::rgb(if active {
                        theme::accent()
                    } else {
                        theme::border_subtle()
                    }))
                    .text_sm()
                    .child(icon("search", px(14.), theme::text_muted()))
                    .child(div().flex_1().min_w_0().child(input))
                    .when(active, |row| {
                        row.child(
                            div()
                                .id("search-clear")
                                .size_5()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(theme::RADIUS_SM)
                                .hover(|style| {
                                    style
                                        .bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                                        .cursor_pointer()
                                })
                                .tooltip(widgets::tooltip_for_action(
                                    "Clear search",
                                    &super::ChatEscape,
                                ))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.clear_search(cx);
                                }))
                                .child(icon("x", px(12.), theme::text_secondary())),
                        )
                    })
            }))
            .child(
                div()
                    .id("session-list")
                    .flex_1()
                    .min_h_0()
                    .pt_6()
                    .child(list),
            )
            .child(self.render_sidebar_footer(cx))
    }

    /// One row of the sidebar list.
    pub(super) fn render_sidebar_entry(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected_session.as_deref();
        let entry = match self.sidebar_entries.get(ix).copied() {
            Some(SidebarEntry::NewTask) => div()
                .id("new-task")
                .w_full()
                .mb_3()
                .px_4()
                .py_1p5()
                .rounded(theme::RADIUS_SM)
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(gpui::rgb(theme::accent()))
                .hover(|style| {
                    style
                        .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                        .cursor_pointer()
                })
                .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
                .tooltip(widgets::tooltip_for_action(
                    "Start a new task",
                    &super::NewTask,
                ))
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.execute_command(ChatCommand::NewTask, window, cx);
                }))
                .child(icon("square-pen", px(16.), theme::accent()))
                .child("New Task")
                .into_any_element(),
            Some(SidebarEntry::ProjectsHeader) => self.render_projects_header(cx),
            Some(SidebarEntry::Task(task)) => {
                let row = self.render_task_row(task, selected, cx);
                // The last row of a section carries the gap before the next
                // section.
                let last_of_section = !task.archived
                    && !matches!(
                        self.sidebar_entries.get(ix + 1),
                        Some(SidebarEntry::Task(task)) if !task.archived
                    );
                div()
                    .when(last_of_section, |row| row.mb_2())
                    .child(row)
                    .into_any_element()
            }
            Some(SidebarEntry::SectionLabel(section)) => {
                let expanded = self.section_expanded(section);
                div()
                    .id(gpui::SharedString::from(section.element_id()))
                    .mt_4()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .mb_1()
                    .px_4()
                    .py_1()
                    .rounded(theme::RADIUS_SM)
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.toggle_section_expanded(section, cx);
                    }))
                    .child(icon(
                        if expanded {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        px(14.),
                        theme::text_secondary(),
                    ))
                    .child(section_label(section.label()))
                    .into_any_element()
            }
            Some(SidebarEntry::Empty(reason)) => {
                let (title, hint) = match reason {
                    SidebarEmpty::NoTasks => ("No tasks yet", "Start one with ⌘N"),
                    SidebarEmpty::NoMatches => ("Nothing matches", "Try another search"),
                };
                div()
                    .mt_6()
                    .px_4()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    // Only a fruitless search gets the glass; an empty inbox
                    // is not something to look for.
                    .when(reason == SidebarEmpty::NoMatches, |column| {
                        column.child(icon("search", px(18.), theme::text_faint()))
                    })
                    .child(div().mt_1().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(hint),
                    )
                    .when(reason == SidebarEmpty::NoMatches, |column| {
                        column.child(
                            widgets::ghost_button("clear-search-empty")
                                .mt_2()
                                .text_xs()
                                .py_1()
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.clear_search(cx);
                                }))
                                .child("Clear search"),
                        )
                    })
                    .into_any_element()
            }
            Some(SidebarEntry::ArchivedHeader) => {
                let expanded = self.archived_expanded;
                let count = self.archived_indices.len();
                div()
                    .id("archived-toggle")
                    .mt_5()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .mb_1()
                    .px_4()
                    .py_1()
                    .rounded(theme::RADIUS_SM)
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.execute_command(ChatCommand::ToggleArchived, window, cx);
                    }))
                    .child(icon(
                        if expanded {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        px(14.),
                        theme::text_secondary(),
                    ))
                    .child(section_label("ARCHIVED"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(count.to_string()),
                    )
                    .into_any_element()
            }
            None => div().into_any_element(),
        };
        if !self.application_vim_enabled {
            return entry;
        }
        let application_selected = self.application_vim_selects_sidebar_row(ix);
        let application_target = self.sidebar_application_target(ix).cloned();
        div()
            .w_full()
            .when_some(application_target, |row, target| {
                row.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        this.select_sidebar_from_pointer(target.clone(), window, cx);
                    }),
                )
            })
            .when(application_selected, |row| {
                row.rounded(theme::RADIUS_SM)
                    .border_l_2()
                    .border_color(gpui::rgb(theme::accent()))
            })
            .child(entry)
            .into_any_element()
    }

    /// The project switcher row: names the scope the task list shows and
    /// opens the menu that picks it. The overflow menu of one project
    /// hangs off its row inside the switcher.
    pub(super) fn render_projects_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self
            .switcher_menu_open
            .then(|| self.render_switcher_menu(cx));
        div()
            .relative()
            .w_full()
            .mb_3()
            .child(
                div()
                    .id("projects-header")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .w_full()
                    .px_4()
                    .py_1()
                    .rounded(theme::RADIUS_SM)
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        cx.stop_propagation();
                        this.toggle_switcher_menu(cx);
                    }))
                    .child(icon("folder", px(16.), theme::text_secondary()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .line_clamp(1)
                            .child(self.sidebar_scope_label.clone()),
                    )
                    .when(self.switcher_menu_open, |row| {
                        row.child(icon("chevron-down", px(14.), theme::text_secondary()))
                    })
                    .when(!self.switcher_menu_open, |row| {
                        row.child(icon("chevron-right", px(14.), theme::text_secondary()))
                    })
                    .child(
                        div()
                            .id("new-project")
                            .size_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme::RADIUS_SM)
                            .hover(|style| {
                                style
                                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                                    .cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.choose_root_dialog(cx);
                            }))
                            .child(icon("folder-plus", px(16.), theme::text_secondary())),
                    ),
            )
            .children(menu)
            .into_any_element()
    }

    /// The menu the project switcher opens: every project, each with an
    /// overflow menu of its own, plus a way back to all projects.
    fn render_switcher_menu(&self, cx: &mut Context<Self>) -> gpui::Deferred {
        let selected = self.sidebar_menu_selection();
        let mut items = vec![
            div()
                .id("switcher-all-projects")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .text_sm()
                .text_color(gpui::rgb(theme::text_primary()))
                .when(selected == Some(0), |row| {
                    row.bg(gpui::rgb(theme::bg_input()))
                })
                .hover(|style| {
                    style
                        .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                        .cursor_pointer()
                })
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.set_sidebar_project_filter(None, cx);
                }))
                .child(div().flex_1().min_w_0().line_clamp(1).child("All projects"))
                .when(self.sidebar_project_filter.is_none(), |row| {
                    row.child(icon("check", px(14.), theme::accent()))
                }),
        ];
        for (index, root) in self.switcher_roots.iter().enumerate() {
            let is_current = self.sidebar_project_filter.as_deref() == Some(root.root.as_str());
            let has_menu = self.project_menu.as_deref() == Some(root.root.as_str());
            let rename_field = self.project_rename_field(&root.root);
            let not_renaming = rename_field.is_none();
            items.push(
                div()
                    .id(root.row_id.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .when(
                        selected.is_some_and(|selected| selected == index + 1),
                        |row| row.bg(gpui::rgb(theme::bg_input())),
                    )
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click({
                        let root = root.root.clone();
                        cx.listener(move |this, _event, _window, cx| {
                            let root = root.clone();
                            this.set_sidebar_project_filter(Some(root), cx);
                        })
                    })
                    .when_some(rename_field, |row, field| row.child(field))
                    .when(not_renaming, |row| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .line_clamp(1)
                                .child(root.name.clone()),
                        )
                    })
                    .when(is_current, |row| {
                        row.child(icon("check", px(14.), theme::accent()))
                    })
                    .child(row_action(
                        root.menu_id.clone(),
                        &root.row_group,
                        "ellipsis",
                        "Project options",
                        {
                            let root = root.root.clone();
                            cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.toggle_project_menu(&root, cx);
                            })
                        },
                    ))
                    .children(has_menu.then(|| self.render_project_menu(&root.root.clone(), cx))),
            );
        }
        let new_project_row = self.switcher_roots.len() + 1;
        items.push(
            div()
                .id("switcher-new-project")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .when(
                    selected.is_some_and(|selected| selected == new_project_row),
                    |row| row.bg(gpui::rgb(theme::bg_input())),
                )
                .hover(|style| {
                    style
                        .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                        .cursor_pointer()
                })
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.set_switcher_menu_open(false, cx);
                    this.choose_root_dialog(cx);
                }))
                .child(icon("folder-plus", px(14.), theme::text_secondary()))
                .child("New project…"),
        );
        gpui::deferred(motion::fade_in(
            div()
                .id("switcher-menu")
                .absolute()
                .top(px(30.))
                .left_0()
                .min_w(px(220.))
                .max_h(px(360.))
                .overflow_y_scroll()
                .p_1()
                .rounded(theme::RADIUS_SM)
                .bg(gpui::rgb(theme::bg_elevated()))
                .border_1()
                .border_color(gpui::rgb(theme::border()))
                .shadow_md()
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                    this.set_switcher_menu_open(false, cx);
                }))
                .children(items),
            "switcher-menu-reveal",
        ))
    }

    /// One task row: title, its project, and hover actions. Archived rows
    /// show a restore button; live rows show an archive button on hover.
    fn render_task_row(
        &self,
        task: SidebarTaskEntry,
        selected: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let SidebarTaskEntry {
            session: index,
            pinned: is_pinned,
            settled,
            archived,
        } = task;
        let row = &self.sidebar_rows[index];
        let is_selected = selected == Some(&*row.id);
        let activity = (!archived)
            .then(|| self.session_activity(&row.id))
            .flatten();
        let session_id = Arc::clone(&row.id);
        let action_id = Arc::clone(&row.id);
        let rename_id = Arc::clone(&row.id);
        let menu_id = Arc::clone(&row.id);
        let pin_id = row.pin_id.clone();
        let rename_field = self.task_rename_field(&row.id);
        let renaming = rename_field.is_some();
        let task_menu_open = self.task_menu.as_deref() == Some(&*row.id);
        div()
            .relative()
            .w_full()
            .id(row.element_id.clone())
            .group(row.group.clone())
            .flex()
            .items_center()
            .gap_1p5()
            .when(archived, |row| row.pl_6())
            .when(!archived, |row| row.pl_8())
            .pr_6()
            .py_1()
            .rounded(theme::RADIUS_MD)
            .text_sm()
            .when(is_selected, |row| {
                row.bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                    .font_weight(gpui::FontWeight::MEDIUM)
            })
            .text_color(gpui::rgb(theme::text_primary()))
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.select_session(&session_id, cx);
            }))
            .when_some(rename_field, |row, field| row.child(field))
            .when(!renaming, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().line_clamp(1).child(row.title.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(gpui::rgb(if is_selected {
                                    theme::text_secondary()
                                } else {
                                    theme::text_muted()
                                }))
                                .line_clamp(1)
                                .child(row.project_name.clone()),
                        ),
                )
            })
            .when_some(activity, |row_element, activity| {
                row_element.child(activity_indicator(&row.spinner_id, activity))
            })
            .when(is_pinned, |row| {
                // Pinned: the always-visible pin is the unpin button itself.
                row.child(
                    div()
                        .id(pin_id.clone())
                        .size_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::RADIUS_SM)
                        .hover(|style| {
                            style
                                .bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                                .cursor_pointer()
                        })
                        .tooltip(widgets::tooltip("Unpin", None))
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.toggle_task_pin(&pin_id, cx);
                        }))
                        .child(icon("pin", px(13.), theme::accent())),
                )
            })
            // The actions float over the right end of the row instead of
            // taking flex space, so the title only loses width while the
            // pointer is over the row. The overlay paints the row hover
            // colour so the covered tail of the title does not bleed
            // through the icons. A pinned row keeps its pin in flow, so the
            // overlay stops short of it.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .when(is_pinned, |overlay| overlay.right(px(26.)))
                    .when(!is_pinned, |overlay| {
                        overlay.right_0().rounded_r(theme::RADIUS_MD)
                    })
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .pl_2()
                    .pr_6()
                    .group_hover(row.group.clone(), |style| {
                        style.bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    })
                    .child(row_action(
                        row.menu_id.clone(),
                        &row.group,
                        "ellipsis",
                        "More",
                        cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.toggle_task_menu(menu_id.as_ref(), cx);
                        }),
                    ))
                    .child(row_action(
                        row.rename_id.clone(),
                        &row.group,
                        "pencil",
                        "Rename",
                        cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.begin_rename(RenameTarget::Task(rename_id.to_string()), cx);
                        }),
                    ))
                    .child(row_action(
                        row.archive_id.clone(),
                        &row.group,
                        if settled { "undo-2" } else { "check" },
                        if settled { "Reopen" } else { "Settle" },
                        cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            if settled {
                                this.unsettle_task(&action_id, cx);
                            } else {
                                this.settle_task(&action_id, cx);
                            }
                        }),
                    )),
            )
            .children(task_menu_open.then(|| self.render_task_menu(task, cx)))
    }

    /// Sidebar footer: just the settings gear. Plan usage stays loaded for
    /// gating image attachments, but is not shown here.
    pub(super) fn render_sidebar_footer(&self, cx: &mut Context<Self>) -> Div {
        let gear = div()
            .id("open-settings")
            .flex_none()
            .size_8()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
            .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
            .tooltip(widgets::tooltip_for_action(
                "Settings",
                &super::OpenAppSettings,
            ))
            .on_click(cx.listener(|this, _event, window, cx| {
                this.execute_command(ChatCommand::OpenSettings, window, cx);
            }))
            .child(icon("settings", px(16.), theme::text_secondary()));
        div().flex().items_center().px_3().py_2().child(gear)
    }
}

/// A spinner while the task runs, a dot once it completed unseen. The
/// spinner keeps the window repainting while any task runs, like the
/// subagent card does; `spinner_id` is prebuilt so render allocates nothing.
fn activity_indicator(spinner_id: &SharedString, activity: SessionActivity) -> AnyElement {
    match activity {
        SessionActivity::Running => spinner_with_id(spinner_id.clone(), px(13.), theme::accent()),
        SessionActivity::CompletedUnread => div()
            .size(px(8.))
            .flex_none()
            .rounded_full()
            .bg(gpui::rgb(theme::status_success()))
            .into_any_element(),
    }
}

/// Small icon button that shows only while the pointer is over its row.
pub(super) fn row_action(
    id: SharedString,
    group: &SharedString,
    icon_name: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size_5()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::RADIUS_SM)
        .opacity(0.)
        .group_hover(group.clone(), |style| style.opacity(1.))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                .cursor_pointer()
        })
        .active(|style| style.bg(gpui::rgb(theme::border())))
        .tooltip(widgets::tooltip(label, None))
        .on_click(on_click)
        .child(icon(icon_name, px(14.), theme::text_secondary()))
}

/// One row of a sidebar popup menu: icon, label, and the hover and
/// Application-Vim backgrounds.
fn popup_menu_row(
    item: SidebarMenuItem,
    selected: bool,
    cx: &mut Context<ChatScreen>,
) -> gpui::Stateful<Div> {
    div()
        .id(item.id)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1p5()
        .text_sm()
        .text_color(gpui::rgb(theme::text_primary()))
        .when(selected, |row| row.bg(gpui::rgb(theme::bg_input())))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                .cursor_pointer()
        })
        .on_click(cx.listener(move |this, _event, _window, cx| {
            cx.stop_propagation();
            (item.on_click)(this, cx);
        }))
        .child(icon(item.icon, px(14.), theme::text_secondary()))
        .child(item.label)
}

/// Field-wise equality for session rows; the summary type has no
/// `PartialEq` of its own.
pub(super) fn session_summary_eq(a: &AgentSessionSummary, b: &AgentSessionSummary) -> bool {
    a.id == b.id
        && a.title == b.title
        && a.project_root == b.project_root
        && a.created_ms == b.created_ms
        && a.updated_ms == b.updated_ms
        && a.message_count == b.message_count
        && a.model == b.model
        && a.mode == b.mode
        && a.web_enabled == b.web_enabled
        && a.archived == b.archived
}

pub(super) fn root_display_name(root: &str) -> String {
    std::path::Path::new(root)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string())
}

/// What a key press means to a modal dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogKey {
    Confirm,
    Cancel,
}

/// Enter confirms and Escape cancels; any modifier means neither.
fn dialog_key(event: &gpui::KeyDownEvent) -> Option<DialogKey> {
    let modifiers = &event.keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
        return None;
    }
    match event.keystroke.key.as_str() {
        "enter" => Some(DialogKey::Confirm),
        "escape" => Some(DialogKey::Cancel),
        _ => None,
    }
}
