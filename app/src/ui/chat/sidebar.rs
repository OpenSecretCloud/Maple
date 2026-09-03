//! The task sidebar: project groups, the virtualized row list, and the
//! actions that pin, rename, fold, trust, archive, or remove a project.
//! Rows and groups are rebuilt when the session list changes, never on
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
use crate::ui::text_input::TextInput;
use crate::ui::theme;

/// Payload of a project header drag: the root being moved.
#[derive(Clone)]
pub(super) struct ProjectDrag {
    pub(super) root: Arc<str>,
}

/// The pill that follows the pointer while a project header is dragged.
struct ProjectDragGhost {
    name: SharedString,
}

impl Render for ProjectDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .shadow_md()
            .text_sm()
            .text_color(gpui::rgb(theme::text_primary()))
            .line_clamp(1)
            .child(self.name.clone())
    }
}

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
    pub(super) title: SharedString,
    /// Display name of the task's project, for archived rows.
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
            title: SharedString::from(session.title.clone()),
            project_name: SharedString::from(project_name.to_string()),
            search: session.title.to_lowercase(),
        }
    }
}

/// One project section of the sidebar with its live tasks, rebuilt with
/// the groups instead of formatted per frame.
pub(super) struct ProjectGroup {
    pub(super) root: Arc<str>,
    pub(super) name: SharedString,
    pub(super) element_id: SharedString,
    /// Hover group that reveals the header's action buttons.
    pub(super) group: SharedString,
    pub(super) pin_id: SharedString,
    pub(super) menu_id: SharedString,
    /// Animation id of the folded header's running indicator.
    pub(super) spinner_id: SharedString,
    /// Indices into `sessions` of the live tasks that pass the filter.
    pub(super) tasks: Vec<usize>,
}

impl ProjectGroup {
    fn build(root: &str, name: String, tasks: Vec<usize>) -> Self {
        Self {
            root: Arc::from(root),
            name: SharedString::from(name),
            element_id: SharedString::from(format!("project-{root}")),
            group: SharedString::from(format!("project-row-{root}")),
            pin_id: SharedString::from(format!("pin-project-{root}")),
            menu_id: SharedString::from(format!("menu-project-{root}")),
            spinner_id: SharedString::from(format!("spinner-project-{root}")),
            tasks,
        }
    }
}

/// One row of the virtualized sidebar list, in display order. Rebuilt
/// with the project groups and when a section folds; the list builds
/// only the rows on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidebarEntry {
    NewTask,
    ProjectsHeader,
    /// Index into `project_groups`.
    Project(usize),
    /// Index into `sessions`.
    Task {
        session: usize,
        archived: bool,
    },
    ArchivedHeader,
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
            input.read(cx).focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// Rebuild the sidebar sections: pinned roots first (when known),
    /// then the saved roots in their persisted order, then roots that only
    /// appear on a stored task. The current project never floats: the
    /// order is static and changes only when the user drags a project.
    /// Called when sessions, roots, names, pins, or the filter change, not
    /// per render.
    pub(super) fn rebuild_project_groups(&mut self) {
        self.sync_sidebar_rows();
        let filter = self.sidebar_filter.as_str();
        let filtering = !filter.is_empty();
        // One pass over the sessions: live tasks per root (in session
        // order), the archived ones, and the roots seen on any task.
        let mut tasks_by_root: HashMap<String, Vec<usize>> = HashMap::new();
        let mut live_roots: Vec<String> = Vec::new();
        let mut session_roots: HashSet<&str> = HashSet::new();
        let mut root_search: HashMap<&str, String> = HashMap::new();
        let mut archived_indices = Vec::new();
        for (index, session) in self.sessions.iter().enumerate() {
            let root = session.project_root.as_str();
            let matches = !filtering
                || self.sidebar_rows[index].search.contains(filter)
                || root_search
                    .entry(root)
                    .or_insert_with(|| self.root_name(root).to_lowercase())
                    .contains(filter);
            if session.archived {
                if matches {
                    archived_indices.push(index);
                }
                continue;
            }
            // Only a live task keeps its project in the sidebar.
            session_roots.insert(root);
            match tasks_by_root.get_mut(root) {
                Some(tasks) => {
                    if matches {
                        tasks.push(index);
                    }
                }
                None => {
                    live_roots.push(root.to_string());
                    tasks_by_root.insert(
                        root.to_string(),
                        if matches { vec![index] } else { Vec::new() },
                    );
                }
            }
        }
        let known = |root: &str| {
            self.recent_roots.iter().any(|candidate| candidate == root)
                || self.project_root.as_deref() == Some(root)
                || session_roots.contains(root)
        };
        let mut seen: HashSet<&str> = HashSet::new();
        let mut roots: Vec<&str> = Vec::new();
        // Roots that only live tasks know about sort alphabetically so
        // their position never moves when sessions change.
        live_roots.sort();
        // The current project stays visible even when nothing else lists
        // it, without floating to the top.
        let ordered = self
            .pinned_roots
            .iter()
            .filter(|root| known(root))
            .chain(self.recent_roots.iter())
            .chain(live_roots.iter())
            .chain(self.project_root.iter());
        for root in ordered {
            if seen.insert(root.as_str()) {
                roots.push(root.as_str());
            }
        }
        let groups: Vec<ProjectGroup> = roots
            .into_iter()
            .map(|root| {
                let tasks = tasks_by_root.remove(root).unwrap_or_default();
                ProjectGroup::build(root, self.root_name(root), tasks)
            })
            // While searching, a project with no matching task is noise.
            .filter(|group| !filtering || !group.tasks.is_empty())
            .collect();
        self.project_groups = groups;
        self.archived_indices = archived_indices;
        self.rebuild_sidebar_entries();
    }

    /// Flatten the groups into list rows, honoring folded projects and
    /// the archived section. Every row is re-measured; the scroll
    /// position is kept.
    pub(super) fn rebuild_sidebar_entries(&mut self) {
        let mut entries = vec![SidebarEntry::NewTask, SidebarEntry::ProjectsHeader];
        for (index, group) in self.project_groups.iter().enumerate() {
            entries.push(SidebarEntry::Project(index));
            if !self.collapsed_roots.contains(&*group.root) {
                entries.extend(group.tasks.iter().map(|&session| SidebarEntry::Task {
                    session,
                    archived: false,
                }));
            }
        }
        if !self.archived_indices.is_empty() {
            entries.push(SidebarEntry::ArchivedHeader);
            if self.archived_expanded {
                entries.extend(
                    self.archived_indices
                        .iter()
                        .map(|&session| SidebarEntry::Task {
                            session,
                            archived: true,
                        }),
                );
            }
        }
        let old_count = self.sidebar_list.item_count();
        self.sidebar_entries = entries;
        if self.application_vim_enabled {
            self.rebuild_sidebar_application_targets();
        }
        self.sidebar_list
            .splice(0..old_count, self.sidebar_entries.len());
        if self.application_vim_enabled {
            self.reconcile_sidebar_application_selection();
        }
    }

    /// A row changed its height (a rename field came or went): let the
    /// list measure again.
    pub(super) fn remeasure_sidebar(&self) {
        let count = self.sidebar_list.item_count();
        self.sidebar_list.splice(0..count, count);
    }

    /// Rebuild the per-session sidebar strings when the session list
    /// moved under them; a plain filter change reuses them.
    pub(super) fn sync_sidebar_rows(&mut self) {
        let fresh = self.sidebar_rows.len() == self.sessions.len()
            && self
                .sidebar_rows
                .iter()
                .zip(&self.sessions)
                .all(|(row, session)| {
                    *row.id == *session.id
                        && row.title.as_ref() == session.title
                        && row.project_name.as_ref() == self.root_name(&session.project_root)
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
            self.rebuild_project_groups();
            cx.notify();
        }
    }

    pub(super) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |input, cx| input.clear(cx));
        }
        self.sidebar_filter.clear();
        self.rebuild_project_groups();
        cx.notify();
    }

    /// Pin or unpin a project root. Pinned roots sort to the top of the
    /// sidebar and persist in the app settings.
    pub(super) fn toggle_pin(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.pinned_roots.iter().any(|pinned| pinned == root) {
            self.pinned_roots.retain(|pinned| pinned != root);
        } else {
            self.pinned_roots.push(root.to_string());
        }
        self.rebuild_project_groups();
        cx.notify();
        let pinned = self.pinned_roots.clone();
        self.persist_settings(move |settings| settings.pinned_roots = pinned, cx);
    }

    /// Track the drop indicator while a project header drag moves over the
    /// header at `index`: pin the insertion point to this row while the
    /// pointer is over it, and clear it when the pointer leaves, so at
    /// most one row shows the line.
    fn update_sidebar_drop_target(
        &mut self,
        index: usize,
        event: &gpui::DragMoveEvent<ProjectDrag>,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position) {
            let mine = self.project_groups.get(index).is_some_and(|group| {
                self.sidebar_drop_target
                    .as_ref()
                    .is_some_and(|(root, _)| root == &group.root)
            });
            if mine {
                self.sidebar_drop_target = None;
                cx.notify();
            }
            return;
        }
        let dragged = event.drag(cx).root.clone();
        let top_half = event.event.position.y < event.bounds.center().y;
        let target = self
            .project_drop_index(&dragged, index, top_half)
            .and_then(|insertion| self.project_drop_target(&dragged, insertion));
        if self.sidebar_drop_target != target {
            self.sidebar_drop_target = target;
            cx.notify();
        }
    }

    /// Insertion point that hovering the group at `over` represents for a
    /// drag of `dragged`: the index the project takes once it is removed
    /// from the groups, clamped to its own section so pinned and unpinned
    /// projects never cross. `None` when the drop would not move anything.
    pub(super) fn project_drop_index(
        &self,
        dragged: &str,
        over: usize,
        top_half: bool,
    ) -> Option<usize> {
        let groups = &self.project_groups;
        if groups.get(over).is_none_or(|group| &*group.root == dragged) {
            return None;
        }
        let source = groups.iter().position(|group| &*group.root == dragged)?;
        let over_remaining = over - usize::from(over > source);
        let pinned = |root: &str| self.pinned_roots.iter().any(|pinned| pinned == root);
        let pinned_remaining = groups
            .iter()
            .filter(|group| pinned(&group.root) && &*group.root != dragged)
            .count();
        let insertion = over_remaining + usize::from(!top_half);
        let insertion = if pinned(dragged) {
            insertion.min(pinned_remaining)
        } else {
            insertion.max(pinned_remaining)
        };
        Some(insertion)
    }

    /// The row that shows the insertion line for `insertion`, the index a
    /// drag of `dragged` produces once the project is removed from the
    /// groups: the line sits above the row at the index, or below the
    /// last one when the project goes to the very end.
    fn project_drop_target(&self, dragged: &str, insertion: usize) -> Option<(Arc<str>, bool)> {
        let remaining: Vec<Arc<str>> = self
            .project_groups
            .iter()
            .map(|group| Arc::clone(&group.root))
            .filter(|root| root.as_ref() != dragged)
            .collect();
        if remaining.is_empty() {
            return None;
        }
        let index = insertion.min(remaining.len() - 1);
        if insertion < remaining.len() {
            Some((Arc::clone(&remaining[index]), true))
        } else {
            Some((Arc::clone(&remaining[remaining.len() - 1]), false))
        }
    }

    /// Re-derive the insertion index from a stored drop target so a list
    /// change mid-drag cannot desync the move.
    pub(super) fn project_insertion_index(
        &self,
        dragged: &str,
        target_root: &str,
        before: bool,
    ) -> Option<usize> {
        let over = self
            .project_groups
            .iter()
            .position(|group| &*group.root == target_root)?;
        self.project_drop_index(dragged, over, before)
    }

    /// Move a dragged project to the insertion point the pointer last
    /// hovered and persist the new order. The optimistic rebuild makes
    /// the drop feel instant; a failed save restores the previous order.
    pub(super) fn reorder_project(&mut self, dragged: &ProjectDrag, cx: &mut Context<Self>) {
        let Some((target_root, before)) = self.sidebar_drop_target.take() else {
            return;
        };
        let Some(insertion) = self.project_insertion_index(&dragged.root, &target_root, before)
        else {
            return;
        };
        let roots: Vec<String> = self
            .project_groups
            .iter()
            .map(|group| group.root.to_string())
            .collect();
        let Some(source) = roots.iter().position(|root| root == dragged.root.as_ref()) else {
            return;
        };
        let mut order = roots.clone();
        order.remove(source);
        order.insert(insertion.min(order.len()), dragged.root.to_string());
        if order == roots {
            return;
        }
        let previous = self.recent_roots.clone();
        self.recent_roots = order.clone();
        self.rebuild_project_groups();
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.save_project_root_order(&user_id, order).await },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(roots) => this.apply_recent_roots(roots),
                    Err(error) => {
                        this.recent_roots = previous;
                        this.rebuild_project_groups();
                        this.notice = Some(error.into());
                    }
                }
                cx.notify();
            },
        );
    }

    /// Apply `update` to the settings file off the UI thread.
    fn persist_settings(
        &self,
        update: impl FnOnce(&mut crate::settings::AppSettings) + Send + 'static,
        _cx: &mut Context<Self>,
    ) {
        crate::settings::update_settings_in_background(update);
    }

    /// Display name for a project root: the saved name, else the folder name.
    pub(super) fn root_name(&self, root: &str) -> String {
        self.project_names
            .get(root)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| root_display_name(root))
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
        self.remeasure_sidebar();
        cx.notify();
    }

    pub(super) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.remeasure_sidebar();
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
        self.remeasure_sidebar();
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
                                this.rebuild_project_groups();
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
                self.rebuild_project_groups();
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
        self.menu_trust = None;
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
                    && this.project_root.as_deref() == Some(status.path.as_str())
                    && status.available
                    && !status.protected_features.is_empty()
                    && status.decision.is_none()
                {
                    this.trust_prompt = Some(status);
                    cx.notify();
                }
            },
        );
    }

    fn set_project_trust(&mut self, path: String, trusted: bool, cx: &mut Context<Self>) {
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
            div()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .when(primary, |button| {
                    button
                        .bg(gpui::rgb(theme::accent()))
                        .text_color(gpui::rgb(theme::bg_app()))
                })
                .when(!primary, |button| {
                    button
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .text_color(gpui::rgb(theme::text_secondary()))
                })
                .when(saving, |button| button.opacity(0.6))
                .when(!saving, |button| {
                    button.hover(|style| style.cursor_pointer().opacity(0.9))
                })
                .child(label)
        };
        let keep_path = status.path.clone();
        let trust_path = status.path.clone();
        div()
            .id("trust-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .bg(theme::scrim())
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(460.))
                    .p_5()
                    .rounded_lg()
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
        let menu_item =
            |id: String, icon_name: &'static str, label: &'static str, on_click: MenuAction| {
                div()
                    .id(SharedString::from(id))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        cx.stop_propagation();
                        on_click(this, cx);
                    }))
                    .child(icon(icon_name, px(14.), theme::text_secondary()))
                    .child(label)
            };
        let rename_root = root.to_string();
        let open_root = root.to_string();
        let remove_root = root.to_string();
        let trust_item = self
            .menu_trust
            .as_ref()
            .filter(|status| status.available && status.path == root)
            .map(|status| {
                let trusted = status.decision == Some(true);
                let path = status.path.clone();
                menu_item(
                    format!("trust-project-{root}"),
                    if trusted { "shield-check" } else { "lock" },
                    if trusted {
                        "Untrust project"
                    } else {
                        "Trust project"
                    },
                    Box::new(move |this, cx| this.set_project_trust(path.clone(), !trusted, cx)),
                )
            });
        gpui::deferred(
            div()
                .id(SharedString::from(format!("project-menu-{root}")))
                .absolute()
                .top(px(30.))
                .right_0()
                .w(px(180.))
                .py_1()
                .rounded_md()
                .bg(gpui::rgb(theme::bg_elevated()))
                .border_1()
                .border_color(gpui::rgb(theme::border()))
                .shadow_md()
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                    this.project_menu = None;
                    cx.notify();
                }))
                .child(menu_item(
                    format!("rename-project-{root}"),
                    "pencil",
                    "Rename project",
                    Box::new(move |this, cx| {
                        this.begin_rename(RenameTarget::Project(rename_root.clone()), cx)
                    }),
                ))
                .child(menu_item(
                    format!("open-project-{root}"),
                    "folder-open",
                    "Open folder",
                    Box::new(move |this, cx| this.open_folder(&open_root, cx)),
                ))
                .children(trust_item)
                .child(menu_item(
                    format!("remove-project-{root}"),
                    "trash-2",
                    "Remove project",
                    Box::new(move |this, cx| this.request_remove_root(&remove_root, cx)),
                )),
        )
    }

    /// Modal that confirms a project removal.
    pub(super) fn render_confirm_remove(
        &self,
        root: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let name = self.root_name(root);
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .when(primary, |button| {
                    button
                        .bg(gpui::rgb(theme::status_error()))
                        .text_color(gpui::rgb(theme::text_primary()))
                })
                .when(!primary, |button| {
                    button
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .text_color(gpui::rgb(theme::text_secondary()))
                })
                .hover(|style| style.cursor_pointer().opacity(0.9))
                .child(label)
        };
        div()
            .id("confirm-remove-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
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
                    .w(px(420.))
                    .p_5()
                    .rounded_lg()
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

    /// Fold or unfold a project's task list without changing task context.
    pub(super) fn toggle_root_collapsed(&mut self, root: &str, cx: &mut Context<Self>) {
        let collapsed = !self.collapsed_roots.contains(root);
        self.set_root_collapsed(root, collapsed, cx);
    }

    /// Deterministically fold or unfold one project. Application Vim uses
    /// this setter for `h`/`l`; pointer clicks retain toggle behavior above.
    pub(super) fn set_root_collapsed(
        &mut self,
        root: &str,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.collapsed_roots.contains(root) == collapsed {
            return;
        }
        if !collapsed {
            self.collapsed_roots.remove(root);
        } else {
            self.collapsed_roots.insert(root.to_string());
        }
        self.rebuild_sidebar_entries();
        cx.notify();
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
            .pinned_roots
            .iter()
            .chain(self.recent_roots.iter())
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
                        this.collapsed_roots.remove(&removed);
                        if this
                            .pinned_roots
                            .iter()
                            .any(|candidate| candidate == &removed)
                        {
                            this.pinned_roots.retain(|candidate| candidate != &removed);
                            let pinned = this.pinned_roots.clone();
                            this.persist_settings(
                                move |settings| settings.pinned_roots = pinned,
                                cx,
                            );
                        }
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
                        this.rebuild_project_groups();
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl_4()
                    .pr_3()
                    .pt_3()
                    .pb_2()
                    .child(wordmark(px(16.), theme::text_primary()))
                    .child(self.render_sidebar_toggle(cx)),
            )
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
                    .rounded_md()
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
                                .rounded_md()
                                .hover(|style| style.cursor_pointer())
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
                    .px_4()
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
                .mb_3()
                .px_2()
                .py_1p5()
                .rounded_md()
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
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.execute_command(ChatCommand::NewTask, window, cx);
                }))
                .child(icon("square-pen", px(16.), theme::accent()))
                .child("New Task")
                .into_any_element(),
            Some(SidebarEntry::ProjectsHeader) => div()
                .flex()
                .items_center()
                .justify_between()
                .mb_3()
                .child(section_label("PROJECTS"))
                .child(
                    div()
                        .id("new-project")
                        .size_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .hover(|style| {
                            style
                                .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                                .cursor_pointer()
                        })
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.choose_root_dialog(cx);
                        }))
                        .child(icon("folder-plus", px(16.), theme::text_secondary())),
                )
                .into_any_element(),
            Some(SidebarEntry::Project(index)) => self.render_project_header(index, cx),
            Some(SidebarEntry::Task { session, archived }) => {
                let row = self.render_task_row(session, selected, archived, cx);
                // The last live row of a project carries the gap before
                // the next section.
                let last_of_group = !archived
                    && !matches!(
                        self.sidebar_entries.get(ix + 1),
                        Some(SidebarEntry::Task {
                            archived: false,
                            ..
                        })
                    );
                div()
                    .when(last_of_group, |row| row.mb_2())
                    .child(row)
                    .into_any_element()
            }
            Some(SidebarEntry::ArchivedHeader) => {
                let expanded = self.archived_expanded;
                let count = self.archived_indices.len();
                div()
                    .id("archived-toggle")
                    .mt_5()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .mb_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
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
            .when_some(application_target, |row, target| {
                row.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        this.select_sidebar_from_pointer(target.clone(), window, cx);
                    }),
                )
            })
            .when(application_selected, |row| {
                row.rounded_md()
                    .border_l_2()
                    .border_color(gpui::rgb(theme::accent()))
            })
            .child(entry)
            .into_any_element()
    }

    /// A project header row with its fold chevron, pin, and overflow
    /// menu. The menu overlays the rows below it.
    pub(super) fn render_project_header(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(group) = self.project_groups.get(index) else {
            return div().into_any_element();
        };
        let root = &group.root;
        let is_current = self.project_root.as_deref() == Some(&**root);
        let is_collapsed = self.collapsed_roots.contains(&**root);
        let is_pinned = self.pinned_roots.iter().any(|pinned| **pinned == **root);
        let activity = is_collapsed
            .then(|| self.aggregate_session_activity(group.tasks.iter().copied()))
            .flatten();
        let rename_field = self.project_rename_field(root);
        let renaming = rename_field.is_some();
        let menu = (self.project_menu.as_deref() == Some(&**root))
            .then(|| self.render_project_menu(root, cx));
        // While searching, the visible projects are a subset of the saved
        // ones, so a drag cannot produce a complete order to persist.
        let draggable = self.sidebar_filter.is_empty();
        let (drop_above, drop_below) = match self.sidebar_drop_target.as_ref() {
            Some((target, false)) if target == root => (true, false),
            Some((target, true)) if target == root => (false, true),
            _ => (false, false),
        };
        div()
            .relative()
            .when(is_collapsed, |column| column.mb_2())
            .when(drop_above, |row| {
                row.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_2()
                        .right_2()
                        .h(px(2.))
                        .rounded_full()
                        .bg(gpui::rgb(theme::accent())),
                )
            })
            .when(drop_below, |row| {
                row.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_2()
                        .right_2()
                        .h(px(2.))
                        .rounded_full()
                        .bg(gpui::rgb(theme::accent())),
                )
            })
            .child(
                div()
                    .id(group.element_id.clone())
                    .group(group.group.clone())
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click({
                        let root = Arc::clone(root);
                        cx.listener(move |this, _event, _window, cx| {
                            this.toggle_root_collapsed(&root, cx);
                        })
                    })
                    .when(draggable, |row| {
                        row.on_drag(
                            ProjectDrag {
                                root: Arc::clone(root),
                            },
                            {
                                let name = group.name.clone();
                                move |_drag, _position, _window, cx| {
                                    cx.new(|_| ProjectDragGhost { name: name.clone() })
                                }
                            },
                        )
                        .on_drag_move::<ProjectDrag>(cx.listener(
                            move |this, event: &gpui::DragMoveEvent<ProjectDrag>, _window, cx| {
                                this.update_sidebar_drop_target(index, event, cx);
                            },
                        ))
                        .on_drop::<ProjectDrag>(cx.listener(
                            |this, drag: &ProjectDrag, _window, cx| {
                                this.reorder_project(drag, cx);
                            },
                        ))
                    })
                    .child(icon(
                        if is_collapsed {
                            "chevron-right"
                        } else {
                            "chevron-down"
                        },
                        px(14.),
                        theme::text_secondary(),
                    ))
                    .child(icon(
                        if is_current { "folder-open" } else { "folder" },
                        px(16.),
                        theme::text_primary(),
                    ))
                    .when_some(rename_field, |row, field| row.child(field))
                    .when(!renaming, |row| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .line_clamp(1)
                                .child(group.name.clone()),
                        )
                    })
                    .when_some(activity, |row, activity| {
                        row.child(activity_indicator(&group.spinner_id, activity))
                    })
                    .when(is_pinned, |row| {
                        // Pinned: the always-visible pin is the unpin
                        // button itself.
                        row.child(
                            div()
                                .id(group.pin_id.clone())
                                .size_5()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|style| style.cursor_pointer())
                                .on_click({
                                    let root = Arc::clone(root);
                                    cx.listener(move |this, _event, _window, cx| {
                                        cx.stop_propagation();
                                        this.toggle_pin(&root, cx);
                                    })
                                })
                                .child(icon("pin", px(13.), theme::accent())),
                        )
                    })
                    .when(!is_pinned, |row| {
                        row.child(row_action(group.pin_id.clone(), &group.group, "pin", {
                            let root = Arc::clone(root);
                            cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.toggle_pin(&root, cx);
                            })
                        }))
                    })
                    .child(row_action(
                        group.menu_id.clone(),
                        &group.group,
                        "ellipsis",
                        {
                            let root = Arc::clone(root);
                            cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.toggle_project_menu(&root, cx);
                            })
                        },
                    )),
            )
            .children(menu)
            .into_any_element()
    }

    /// One task row. Archived rows show the project name under the title
    /// and a restore button; live rows show an archive button on hover.
    fn render_task_row(
        &self,
        index: usize,
        selected: Option<&str>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let row = &self.sidebar_rows[index];
        let is_selected = selected == Some(&*row.id);
        let activity = (!archived)
            .then(|| self.session_activity(&row.id))
            .flatten();
        let session_id = Arc::clone(&row.id);
        let action_id = Arc::clone(&row.id);
        let rename_id = Arc::clone(&row.id);
        let rename_field = self.task_rename_field(&row.id);
        let renaming = rename_field.is_some();
        div()
            .id(row.element_id.clone())
            .group(row.group.clone())
            .flex()
            .items_center()
            .gap_1p5()
            .when(archived, |row| row.pl_2())
            .when(!archived, |row| row.pl_8())
            .pr_2()
            .py_1()
            .rounded_lg()
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
                        .when(archived, |column| {
                            column.child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(theme::text_muted()))
                                    .line_clamp(1)
                                    .child(row.project_name.clone()),
                            )
                        }),
                )
            })
            .when_some(activity, |row_element, activity| {
                row_element.child(activity_indicator(&row.spinner_id, activity))
            })
            .child(row_action(
                row.rename_id.clone(),
                &row.group,
                "pencil",
                cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.begin_rename(RenameTarget::Task(rename_id.to_string()), cx);
                }),
            ))
            .child(row_action(
                row.archive_id.clone(),
                &row.group,
                if archived {
                    "archive-restore"
                } else {
                    "archive"
                },
                cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.set_session_archived(&action_id, !archived, cx);
                }),
            ))
    }

    /// Sidebar footer: just the settings gear. Plan usage stays loaded for
    /// gating image attachments, but is not shown here.
    pub(super) fn render_sidebar_footer(&self, cx: &mut Context<Self>) -> Div {
        let gear = div()
            .id("open-settings")
            .flex_none()
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .size_9()
            .rounded_full()
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
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
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size_5()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .opacity(0.)
        .group_hover(group.clone(), |style| style.opacity(1.))
        .hover(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
        .on_click(on_click)
        .child(icon(icon_name, px(14.), theme::text_secondary()))
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
