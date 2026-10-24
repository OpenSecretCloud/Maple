//! Bridge the host's acknowledged remote-update queue to this computer's
//! existing desktop installer. The client computer never enters this path.

use std::{sync::Arc, time::Duration};

use gpui::Context;
use tokio::sync::Mutex;

use crate::{
    backend::AgentBackend,
    remote::update::{ReleaseIdentity, RemoteUpdateStatus},
    remote_updates::{
        AttachRequest, CompleteRequest, DesktopCommand, DesktopLease, PublishRequest, TakeRequest,
    },
    update::{Controller, Snapshot, State},
};

use super::{
    DesktopLifecycle, MapleApp, cancel_quit, persist_window_state, restart_installed_update,
};

#[derive(Default, Clone)]
pub(super) struct Bridge(Arc<Mutex<Option<Registration>>>);

struct Registration {
    account: String,
    lease: DesktopLease,
    published: Option<RemoteUpdateStatus>,
}

pub(super) struct Pending {
    account: String,
    lease: String,
    command: DesktopCommand,
}

impl Bridge {
    /// Called by the existing five-second local status pump. No new timer or
    /// long-polling host request keeps an account/admission guard alive.
    pub(super) async fn poll(
        &self,
        backend: Arc<AgentBackend>,
        account: Option<String>,
        status: RemoteUpdateStatus,
    ) -> Option<Pending> {
        let mut registration = self.0.lock().await;
        let Some(account) = account.filter(|_| !backend.is_retired_after_update()) else {
            *registration = None;
            return None;
        };
        if registration
            .as_ref()
            .is_some_and(|old| old.account != account)
        {
            *registration = None;
        }
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            if registration.is_none() {
                let lease = backend
                    .remote_setting(
                        "remote_update_attach",
                        AttachRequest {
                            expected_account: account.clone(),
                        },
                    )
                    .await?;
                *registration = Some(Registration {
                    account: account.clone(),
                    lease,
                    published: None,
                });
            }
            let current = registration.as_mut().expect("attached desktop lease");
            if current.published.as_ref() != Some(&status) {
                backend
                    .remote_setting::<()>(
                        "remote_update_publish",
                        PublishRequest {
                            expected_account: account.clone(),
                            lease: current.lease.id.clone(),
                            status: status.clone(),
                        },
                    )
                    .await?;
                current.published = Some(status);
            }
            // Taking is immediate and renews the lease even with no command.
            let command: Option<DesktopCommand> = backend
                .remote_setting(
                    "remote_update_take",
                    TakeRequest {
                        expected_account: account.clone(),
                        lease: current.lease.id.clone(),
                    },
                )
                .await?;
            Ok::<_, String>(command.map(|command| Pending {
                account,
                lease: current.lease.id.clone(),
                command,
            }))
        })
        .await;
        match result {
            Ok(Ok(command)) => command,
            _ => {
                *registration = None;
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Admission {
    store_ready: bool,
    store_open: bool,
    local_backend: bool,
    prompt_pending: bool,
    closing: bool,
}

fn may_begin(
    current_account: Option<&str>,
    expected_account: &str,
    snapshot: &Snapshot,
    target: &ReleaseIdentity,
    admission: Admission,
) -> bool {
    current_account == Some(expected_account)
        && admission.store_ready
        && admission.store_open
        && admission.local_backend
        && !admission.prompt_pending
        && !admission.closing
        && snapshot.state == State::Ready
        && snapshot.remote_candidate.as_ref() == Some(target)
}

async fn failed(backend: &AgentBackend, pending: Pending) {
    if backend.is_retired_after_update() {
        return;
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), backend.remote_setting::<()>(
        "remote_update_complete", CompleteRequest {
            expected_account: pending.account,
            lease: pending.lease,
            operation_id: pending.command.operation_id,
            error: Some("The remote update did not complete. Open Maple on that computer to review it before trying again.".into()),
        },
    )).await;
}

impl MapleApp {
    pub(super) fn begin_remote_install(&mut self, pending: Pending, cx: &mut Context<Self>) {
        let lifecycle = cx.try_global::<DesktopLifecycle>().cloned();
        let allowed = lifecycle.as_ref().is_some_and(|lifecycle| {
            may_begin(
                self.user_id.as_deref(),
                &pending.account,
                &self.updater.snapshot(),
                &pending.command.target,
                Admission {
                    store_ready: self.store_ready,
                    store_open: self.backend.store_state() == maple_agent::store::StoreState::Open,
                    local_backend: self.backend.remote_host().is_none(),
                    prompt_pending: self.restart_prompt_pending,
                    closing: lifecycle.closing.get(),
                },
            )
        });
        if !allowed {
            let backend = self.backend.clone();
            drop(
                self.backend
                    .spawn(async move { failed(&backend, pending).await }),
            );
            return;
        }
        // This is the execution computer's lifecycle. The controller's local
        // updater, selected remote chat and other paired computers are untouched.
        lifecycle
            .expect("checked desktop lifecycle")
            .closing
            .set(true);
        self.restart_prompt_pending = true;
        persist_window_state();
        let updater: Controller = self.updater.clone();
        let backend = self.backend.clone();
        let install_backend = backend.clone();
        let target = pending.command.target.clone();
        let operation = pending.command.operation_id.clone();
        let bridge = cx.spawn(async move |this, cx| {
            let controller = updater.clone();
            let installed = backend
                .spawn(async move {
                    controller
                        .install_remote(install_backend, target, operation)
                        .await
                })
                .await;
            // Once committed, do not issue any old-build host RPC, even if the
            // task failed after replacement. The retained controller path and
            // retirement fence are the restart authority.
            let committed = updater.restart_path();
            if committed.is_none() && !backend.is_retired_after_update() {
                let completing = backend.clone();
                let _ = backend
                    .spawn(async move { failed(&completing, pending).await })
                    .await;
            }
            let _ = this.update(cx, |this, cx| {
                this.restart_prompt_pending = false;
                if let Some(path) = committed {
                    restart_installed_update(path, cx);
                } else {
                    // Errors and changed/withdrawn offers retain the old app.
                    // Never install the new offer without a fresh confirmation.
                    let _ = installed;
                    cancel_quit(cx);
                }
            });
        });
        crate::ui::task::retain(&self.remote_bridges, bridge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> ReleaseIdentity {
        ReleaseIdentity {
            version: "0.0.7".into(),
            build: "551".into(),
            source_sha: "a".repeat(40),
            manifest_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn acknowledged_command_cannot_cross_accounts_hosts_or_changed_candidates() {
        let target = target();
        let mut snapshot = Snapshot::initial();
        snapshot.state = State::Ready;
        snapshot.remote_candidate = Some(target.clone());
        let admission = Admission {
            store_ready: true,
            store_open: true,
            local_backend: true,
            prompt_pending: false,
            closing: false,
        };
        assert!(may_begin(
            Some("account-a"),
            "account-a",
            &snapshot,
            &target,
            admission
        ));
        assert!(!may_begin(
            Some("account-b"),
            "account-a",
            &snapshot,
            &target,
            admission
        ));
        assert!(!may_begin(
            Some("account-a"),
            "account-a",
            &snapshot,
            &target,
            Admission {
                local_backend: false,
                ..admission
            }
        ));
        assert!(!may_begin(
            Some("account-a"),
            "account-a",
            &snapshot,
            &target,
            Admission {
                store_open: false,
                ..admission
            }
        ));
        assert!(!may_begin(
            Some("account-a"),
            "account-a",
            &snapshot,
            &target,
            Admission {
                closing: true,
                ..admission
            }
        ));
        let mut changed = target.clone();
        changed.manifest_sha256 = "c".repeat(64);
        snapshot.remote_candidate = Some(changed);
        assert!(!may_begin(
            Some("account-a"),
            "account-a",
            &snapshot,
            &target,
            admission
        ));
    }
}
