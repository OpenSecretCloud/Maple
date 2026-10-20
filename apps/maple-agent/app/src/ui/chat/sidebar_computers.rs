//! The normal sidebar list, grouped by computer and folder when paired hosts
//! exist. This is a summary projection, never a second task store.
use super::*;
use crate::ui::chat::computers::{Computer, Destination, Navigate};

pub(super) struct ProjectHeading {
    request: Navigate,
    id: SharedString,
    label: SharedString,
    remote: bool,
}
pub(super) struct OtherTask {
    pub(super) request: Navigate,
    pub(super) archived: bool,
    id: SharedString,
    title: SharedString,
    activity: Option<SessionActivity>,
    pinned: bool,
}

impl Sidebar {
    pub(in crate::ui::chat) fn set_computers(
        &mut self,
        computers: Arc<Vec<Computer>>,
        cx: &mut Context<Self>,
    ) {
        self.computers = computers;
        self.rebuild_entries();
        self.list.remeasure_items(0..self.entries.len());
        cx.notify();
    }

    pub(super) fn computer_entries(&mut self) -> Vec<SidebarEntry> {
        self.computer_projects.clear();
        self.other_tasks.clear();
        self.computer_archived_count = 0;
        let mut entries = vec![SidebarEntry::NewTask, SidebarEntry::ProjectsHeader];
        for (computer_index, computer) in self.computers.iter().enumerate() {
            let own = computer.key.as_deref() == self.backend.remote_host();
            if self.project_filter.is_some() && !own {
                continue;
            }
            self.computer_archived_count += computer
                .sessions
                .iter()
                .filter(|s| {
                    s.state == AgentTaskState::Archived
                        && self
                            .project_filter
                            .as_ref()
                            .is_none_or(|root| root == &s.project_root)
                })
                .count();
            if computer.projects.is_empty() || computer.error.is_some() {
                entries.push(SidebarEntry::Computer(computer_index));
            }
            for project in &computer.projects {
                if own
                    && self
                        .project_filter
                        .as_ref()
                        .is_some_and(|p| p != &project.path)
                {
                    continue;
                }
                let matches_group = self.filter.is_empty()
                    || computer.label.to_lowercase().contains(&self.filter)
                    || project.label.to_lowercase().contains(&self.filter);
                let mut tasks = computer
                    .sessions
                    .iter()
                    .filter(|task| {
                        task.project_root == project.path
                            && (task.message_count > 0 || computer.running.contains(&task.id))
                            && (task.state != AgentTaskState::Archived || self.archived_expanded)
                            && (matches_group || task.title.to_lowercase().contains(&self.filter))
                    })
                    .collect::<Vec<_>>();
                tasks.sort_by_key(|s| {
                    (
                        s.state == AgentTaskState::Archived,
                        s.state == AgentTaskState::Settled,
                        computer
                            .pinned
                            .iter()
                            .position(|id| id == &s.id)
                            .unwrap_or(usize::MAX),
                        std::cmp::Reverse(s.updated_ms),
                    )
                });
                if tasks.is_empty() {
                    continue;
                }
                let heading = self.computer_projects.len();
                self.computer_projects.push(ProjectHeading {
                    request: Navigate {
                        computer: computer.key.clone(),
                        destination: Destination::Project(project.path.clone()),
                    },
                    id: format!("computer-{computer_index}-folder-{heading}").into(),
                    label: format!("{} · {}", computer.label, project.label).into(),
                    remote: computer.key.is_some(),
                });
                entries.push(SidebarEntry::ComputerProject(heading));
                for task in tasks {
                    if own {
                        if let Some(index) = self.sessions.iter().position(|s| s.id == task.id) {
                            entries.push(SidebarEntry::Task(SidebarTaskEntry {
                                session: index,
                                pinned: self.pinned_tasks.contains(&task.id),
                                settled: task.state == AgentTaskState::Settled,
                                archived: task.state == AgentTaskState::Archived,
                            }));
                        }
                    } else {
                        let index = self.other_tasks.len();
                        self.other_tasks.push(OtherTask {
                            archived: task.state == AgentTaskState::Archived,
                            request: Navigate {
                                computer: computer.key.clone(),
                                destination: Destination::Task(task.id.clone()),
                            },
                            id: format!("computer-{computer_index}-task-{}", task.id).into(),
                            title: task.title.clone().into(),
                            pinned: computer.pinned.contains(&task.id),
                            activity: if computer.running.contains(&task.id) {
                                Some(SessionActivity::Running)
                            } else if computer.unread.contains(&task.id) {
                                Some(SessionActivity::CompletedUnread)
                            } else {
                                None
                            },
                        });
                        entries.push(SidebarEntry::OtherTask(index));
                    }
                }
            }
        }
        if self.computer_archived_count > 0 {
            entries.push(SidebarEntry::ArchivedHeader);
        }
        entries
    }

    pub(in crate::ui::chat) fn computer_target(
        &self,
        entry: SidebarEntry,
    ) -> Option<SidebarTarget> {
        let request = match entry {
            SidebarEntry::ComputerProject(index) => {
                self.computer_projects.get(index)?.request.clone()
            }
            SidebarEntry::OtherTask(index) => self.other_tasks.get(index)?.request.clone(),
            SidebarEntry::Computer(index) => Navigate {
                computer: self.computers.get(index)?.key.clone(),
                destination: Destination::Overview,
            },
            _ => return None,
        };
        Some(SidebarTarget::Computer(request))
    }

    pub(super) fn render_computer_entry(
        &self,
        entry: SidebarEntry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match entry {
            SidebarEntry::ComputerProject(index) => {
                let project = &self.computer_projects[index];
                let request = project.request.clone();
                div()
                    .id(project.id.clone())
                    .role(gpui::Role::Button)
                    .aria_label(project.label.clone())
                    .mt_3()
                    .px_4()
                    .py_1p5()
                    .flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .when(selected, |row| {
                        row.aria_active_descendant()
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    })
                    .hover(|s| {
                        s.bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SidebarEvent::Navigate(request.clone()))
                    }))
                    .child(icon(
                        if project.remote { "globe" } else { "folder" },
                        px(14.),
                        theme::text_muted(),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(project.label.clone()),
                    )
                    .into_any_element()
            }
            SidebarEntry::OtherTask(index) => {
                let task = &self.other_tasks[index];
                let request = task.request.clone();
                div()
                    .id(task.id.clone())
                    .role(gpui::Role::ListBoxOption)
                    .aria_label(task.title.clone())
                    .aria_selected(false)
                    .pl_8()
                    .pr_6()
                    .py_1()
                    .flex()
                    .gap_2()
                    .items_center()
                    .rounded(theme::RADIUS_MD)
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .when(selected, |row| {
                        row.aria_active_descendant()
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    })
                    .hover(|s| {
                        s.bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SidebarEvent::Navigate(request.clone()))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .line_clamp(1)
                            .text_ellipsis()
                            .child(task.title.clone()),
                    )
                    .when_some(task.activity, |row, activity| {
                        row.child(activity_indicator(&task.id, activity))
                    })
                    .when(task.pinned, |row| {
                        row.child(icon("pin", px(13.), theme::accent()))
                    })
                    .into_any_element()
            }
            SidebarEntry::Computer(index) => {
                let computer = &self.computers[index];
                let request = Navigate {
                    computer: computer.key.clone(),
                    destination: Destination::Overview,
                };
                div()
                    .id(SharedString::from(format!("computer-{index}")))
                    .role(gpui::Role::Button)
                    .aria_label(computer.label.clone())
                    .mt_3()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .hover(|s| {
                        s.bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SidebarEvent::Navigate(request.clone()))
                    }))
                    .child(computer.label.clone())
                    .child(computer.error.clone().unwrap_or_else(|| {
                        if computer.loading {
                            "Connecting…"
                        } else {
                            "No projects yet"
                        }
                        .into()
                    }))
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    pub(super) fn computer_switcher_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Menu<Self> {
        let mut menu = Menu::new("switcher-menu", px(330.))
            .label("Projects")
            .max_height(px(440.))
            .application_vim(self.application_vim_enabled)
            .item(
                MenuItem::new(ALL_PROJECTS_ID, "All projects", |this: &mut Self, _, cx| {
                    this.set_project_filter(None, cx)
                })
                .current(self.project_filter.is_none()),
            );
        for (index, computer) in self.computers.iter().enumerate() {
            menu = menu.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(computer.label.clone()),
            );
            for (project_index, project) in computer.projects.iter().enumerate() {
                // Keep the normal project rename/trust/removal controls for
                // this computer; foreign project rows navigate to their owner.
                if computer.key.as_deref() == self.backend.remote_host()
                    && let Some(root) = self
                        .switcher_roots
                        .iter()
                        .find(|root| root.root == project.path)
                {
                    let pick = root.root.clone();
                    let group = root.row_group.clone();
                    let mut item = MenuItem::new(
                        root.row_id.clone(),
                        root.name.clone(),
                        move |this: &mut Self, _, cx| {
                            this.set_project_filter(Some(pick.clone()), cx);
                        },
                    )
                    .current(self.project_filter.as_deref() == Some(root.root.as_str()))
                    .style(move |row| row.group(group))
                    .trailing(self.project_menu_trigger(root, window, cx));
                    if let Some(field) = self.project_rename_field(&root.root) {
                        item = item.content(field);
                    }
                    menu = menu.item(item);
                    continue;
                }
                let request = Navigate {
                    computer: computer.key.clone(),
                    destination: Destination::Project(project.path.clone()),
                };
                menu = menu.item(
                    MenuItem::new(
                        SharedString::from(format!(
                            "switcher-computer-{index}-project-{project_index}"
                        )),
                        project.label.clone(),
                        move |_: &mut Self, _, cx| cx.emit(SidebarEvent::Navigate(request.clone())),
                    )
                    .icon(if computer.key.is_some() {
                        "globe"
                    } else {
                        "folder"
                    })
                    .current(
                        computer.key.as_deref() == self.backend.remote_host()
                            && self.project_filter.as_deref() == Some(project.path.as_str()),
                    ),
                );
            }
            let request = Navigate {
                computer: computer.key.clone(),
                destination: Destination::NewProject,
            };
            menu = menu.item(
                MenuItem::new(
                    SharedString::from(format!("switcher-computer-{index}-new-project")),
                    "New project…",
                    move |_: &mut Self, _, cx| cx.emit(SidebarEvent::Navigate(request.clone())),
                )
                .icon("folder-plus"),
            );
        }
        menu
    }
}
