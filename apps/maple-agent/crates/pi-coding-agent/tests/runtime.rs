//! Pi's session runtime: new, stored, forked and imported sessions in place of the
//! current one.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::FutureExt;
use pi_ai::faux::FauxProvider;
use pi_ai::{Message, UserMessage};
use pi_coding_agent::extensions::{
    BeforeSwitchResult, ForkPosition, SessionBeforeFork, SessionBeforeSwitch, SessionShutdown,
    SessionStart, extension,
};
use pi_coding_agent::resources::ResourcePaths;
use pi_coding_agent::runtime::{
    ForkOptions, NewSessionOptions, SwitchSessionOptions, create_agent_session_runtime,
};
use pi_coding_agent::session::{EntryKind, SessionManager, default_session_dir};
use pi_coding_agent::{
    AgentSession, AgentSessionError, AgentSessionRuntime, CreateAgentSessionOptions, ModelRegistry,
    PromptOptions, RuntimeTarget, SessionMessage, StaticKeys, create_agent_session,
};

type Log = Arc<Mutex<Vec<String>>>;

fn name(file: &Option<PathBuf>) -> String {
    file.as_ref()
        .and_then(|file| file.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "-".into())
}

struct Host {
    _root: tempfile::TempDir,
    agent: PathBuf,
    project: PathBuf,
    faux: FauxProvider,
    models: ModelRegistry,
    log: Log,
    refuse_switch: Arc<AtomicBool>,
}

impl Host {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let agent = root.path().join("agent");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let faux = FauxProvider::new();
        let keys = HashMap::from([("faux".to_string(), "key".to_string())]);
        let models = ModelRegistry::new(Arc::new(StaticKeys(keys)));
        models.register_api("faux", Arc::new(faux.clone()));
        models.register_models([faux.model()]);
        Self {
            _root: root,
            agent,
            project,
            faux,
            models,
            log: Log::default(),
            refuse_switch: Arc::default(),
        }
    }

    fn log(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    /// A runtime whose sessions have an extension that records the session events
    /// and a `/fresh` command that starts a new session.
    async fn runtime(&self, session: Option<SessionManager>) -> AgentSessionRuntime {
        let (agent, models, log, refuse) = (
            self.agent.clone(),
            self.models.clone(),
            self.log.clone(),
            self.refuse_switch.clone(),
        );
        let factory = move |target: RuntimeTarget| {
            let mut paths = ResourcePaths::new(&agent, ".maple");
            paths.home_dir = None;
            let mut options = CreateAgentSessionOptions::new("Maple", paths, models.clone());
            options.cwd = Some(target.cwd);
            options.session = Some(target.session);
            options.session_start = target.session_start;
            options.builtin_tools = Some(Vec::new());
            options.extensions = vec![recorder(log.clone(), refuse.clone())];
            create_agent_session(options)
        };
        let session = session.unwrap_or_else(|| {
            SessionManager::create_in(
                self.project.to_string_lossy(),
                &default_session_dir(&self.project, &self.agent),
            )
        });
        create_agent_session_runtime(
            factory,
            RuntimeTarget {
                cwd: self.project.clone(),
                session,
                session_start: SessionStart::default(),
            },
        )
        .await
        .unwrap()
    }

    async fn chat(&self, session: &AgentSession, text: &str) {
        self.faux.push_text(&format!("re: {text}"));
        session
            .prompt(text, PromptOptions::default())
            .await
            .unwrap();
    }
}

fn recorder(
    log: Log,
    refuse_switch: Arc<AtomicBool>,
) -> Arc<dyn pi_coding_agent::extensions::Extension> {
    extension("recorder", move |api| {
        let sink = log.clone();
        api.on(move |event: SessionStart, _ctx| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(format!(
                    "start:{:?}:{}",
                    event.reason,
                    name(&event.previous_session_file)
                ));
                Ok(())
            }
        });
        let sink = log.clone();
        api.on(move |event: SessionShutdown, _ctx| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(format!(
                    "shutdown:{:?}:{}",
                    event.reason,
                    name(&event.target_session_file)
                ));
                Ok(())
            }
        });
        let (sink, refuse) = (log.clone(), refuse_switch.clone());
        api.on(move |event: SessionBeforeSwitch, _ctx| {
            let (sink, refuse) = (sink.clone(), refuse.clone());
            async move {
                sink.lock().unwrap().push(format!(
                    "before_switch:{:?}:{}",
                    event.reason,
                    name(&event.target_session_file)
                ));
                Ok(BeforeSwitchResult {
                    cancel: refuse.load(Ordering::SeqCst),
                })
            }
        });
        let sink = log.clone();
        api.on(move |event: SessionBeforeFork, _ctx| {
            let sink = sink.clone();
            async move {
                sink.lock()
                    .unwrap()
                    .push(format!("before_fork:{:?}", event.position));
                Ok(Default::default())
            }
        });
        let sink = log.clone();
        api.register_command("fresh", "Start over", move |_args, ctx| {
            let sink = sink.clone();
            async move {
                let options = NewSessionOptions {
                    with_session: Some(Box::new(move |ctx| {
                        async move {
                            sink.lock()
                                .unwrap()
                                .push(format!("with_session:{}", name(&ctx.session_file())));
                        }
                        .boxed()
                    })),
                    ..NewSessionOptions::default()
                };
                ctx.new_session(options).await?;
                Ok(())
            }
        });
    })
}

#[tokio::test]
async fn a_new_session_replaces_the_current_one() {
    let host = Host::new();
    let runtime = host.runtime(None).await;
    let first = runtime.session();
    host.chat(&first, "hello").await;
    let first_file = first.session_file().unwrap();
    assert_eq!(host.log(), ["start:Startup:-"]);
    let rebound = Arc::new(Mutex::new(Vec::new()));
    let sink = rebound.clone();
    runtime.set_rebind_session(move |session| {
        sink.lock().unwrap().push(session.session_id());
        async {}.boxed()
    });

    runtime
        .new_session(NewSessionOptions {
            parent_session: Some("parent.jsonl".into()),
            setup: Some(Box::new(|tree| {
                tree.append_session_info(Some("Planned".into()));
            })),
            ..NewSessionOptions::default()
        })
        .await
        .unwrap();
    let second = runtime.session();
    let second_file = second.session_file().unwrap();
    assert_ne!(second.session_id(), first.session_id());
    // Kept next to the one before, as in Pi.
    assert_eq!(second_file.parent(), first_file.parent());
    let first_name = name(&Some(first_file.clone()));
    let second_name = name(&Some(second_file.clone()));
    assert_eq!(
        host.log(),
        [
            "before_switch:New:-".to_string(),
            format!("shutdown:New:{second_name}"),
            format!("start:New:{first_name}"),
        ]
    );
    assert_eq!(*rebound.lock().unwrap(), [second.session_id()]);
    assert_eq!(second.session_name().as_deref(), Some("Planned"));
    assert_eq!(
        second.with_session(|tree| tree.header().parent_session.clone()),
        Some("parent.jsonl".into())
    );

    // An extension can keep the session.
    host.refuse_switch.store(true, Ordering::SeqCst);
    assert!(matches!(
        runtime.new_session(NewSessionOptions::default()).await,
        Err(AgentSessionError::Cancelled)
    ));
    assert_eq!(runtime.session().session_id(), second.session_id());
    host.refuse_switch.store(false, Ordering::SeqCst);
    host.log();

    // A stored session comes back with its conversation.
    runtime
        .switch_session(&first_file, SwitchSessionOptions::default())
        .await
        .unwrap();
    let resumed = runtime.session();
    assert_eq!(resumed.session_id(), first.session_id());
    assert!(
        resumed
            .messages()
            .iter()
            .any(|message| message.text() == "hello")
    );
    assert_eq!(
        host.log(),
        [
            format!("before_switch:Resume:{first_name}"),
            format!("shutdown:Resume:{first_name}"),
            format!("start:Resume:{second_name}"),
        ]
    );

    runtime.dispose().await;
    assert_eq!(host.log(), ["shutdown:Quit:-"]);
}

#[tokio::test]
async fn a_command_can_start_a_new_session() {
    let host = Host::new();
    let runtime = host.runtime(None).await;
    let first = runtime.session();
    host.chat(&first, "hello").await;
    host.log();

    first
        .prompt("/fresh", PromptOptions::default())
        .await
        .unwrap();
    let second = runtime.session();
    assert_ne!(second.session_id(), first.session_id());
    let log = host.log();
    assert_eq!(log.first().map(String::as_str), Some("before_switch:New:-"));
    assert_eq!(
        log.last().cloned(),
        Some(format!("with_session:{}", name(&second.session_file())))
    );

    // The session left behind no longer replaces the runtime's.
    first
        .prompt("/fresh", PromptOptions::default())
        .await
        .unwrap();
    assert_eq!(runtime.session().session_id(), second.session_id());
    assert!(host.log().is_empty());
}

#[tokio::test]
async fn a_fork_replaces_the_session_as_in_pi() {
    let host = Host::new();
    let runtime = host.runtime(None).await;
    let session = runtime.session();
    host.chat(&session, "first").await;
    host.chat(&session, "second").await;
    let file = session.session_file().unwrap();
    let second = session.with_session(|tree| {
        tree.branch()
            .iter()
            .rev()
            .find(|entry| matches!(&entry.kind, EntryKind::Message { message } if message.text() == "second"))
            .unwrap()
            .id
            .clone()
    });
    host.log();

    let edited = runtime.fork(&second, ForkOptions::default()).await.unwrap();
    assert_eq!(edited.as_deref(), Some("second"));
    let fork = runtime.session();
    assert_ne!(fork.session_id(), session.session_id());
    let fork_file = fork.session_file().unwrap();
    assert_eq!(fork_file.parent(), file.parent());
    assert!(fork_file.exists());
    assert_eq!(
        fork.with_session(|tree| tree.header().parent_session.clone()),
        Some(file.to_string_lossy().into_owned())
    );
    assert!(
        fork.messages()
            .iter()
            .any(|message| message.text() == "first")
    );
    assert!(
        !fork
            .messages()
            .iter()
            .any(|message| message.text() == "second")
    );
    assert_eq!(
        host.log(),
        [
            "before_fork:Before".to_string(),
            format!("shutdown:Fork:{}", name(&Some(fork_file.clone()))),
            format!("start:Fork:{}", name(&Some(file.clone()))),
        ]
    );
    // The session it came from is as it was.
    let (_, entries) = pi_coding_agent::store::JsonlStore::load(&file)
        .unwrap()
        .unwrap();
    assert!(entries.iter().any(
        |entry| matches!(&entry.kind, EntryKind::Message { message } if message.text() == "second")
    ));
}

#[tokio::test]
async fn a_session_kept_in_memory_forks_in_memory() {
    let host = Host::new();
    let runtime = host
        .runtime(Some(SessionManager::in_memory(
            host.project.to_string_lossy(),
        )))
        .await;
    let session = runtime.session();
    host.chat(&session, "first").await;
    let leaf = session.with_session(|tree| tree.leaf_id().unwrap().to_string());
    let edited = runtime
        .fork(
            &leaf,
            ForkOptions {
                position: ForkPosition::At,
                with_session: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(edited, None);
    let fork = runtime.session();
    assert_eq!(fork.session_file(), None);
    assert!(
        fork.messages()
            .iter()
            .any(|message| message.text() == "re: first")
    );
    assert_eq!(
        fork.with_session(|tree| tree.header().parent_session.clone()),
        None
    );
}

#[tokio::test]
async fn an_unsaved_session_cannot_be_forked_at_an_entry() {
    let host = Host::new();
    let runtime = host.runtime(None).await;
    // The model and thinking level are recorded, but nothing is written before a
    // conversation.
    let entry = runtime
        .session()
        .with_session(|tree| tree.leaf_id().unwrap().to_string());
    let result = runtime
        .fork(
            &entry,
            ForkOptions {
                position: ForkPosition::At,
                with_session: None,
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(AgentSessionError::Failed(message)) if message.starts_with("This session has not been saved yet.")
    ));
}

#[tokio::test]
async fn a_session_file_is_imported_next_to_the_current_one() {
    let host = Host::new();
    // A session from elsewhere, with a conversation.
    let elsewhere = host._root.path().join("elsewhere");
    let source = {
        let runtime = host
            .runtime(Some(SessionManager::create_in(
                host.project.to_string_lossy(),
                &elsewhere,
            )))
            .await;
        host.chat(&runtime.session(), "imported").await;
        runtime.session().session_file().unwrap()
    };
    let runtime = host.runtime(None).await;
    host.chat(&runtime.session(), "current").await;
    let dir = runtime
        .session()
        .session_file()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    host.log();

    runtime.import_from_jsonl(&source, None).await.unwrap();
    let imported = runtime.session();
    let copy = imported.session_file().unwrap();
    assert_eq!(copy, dir.join(source.file_name().unwrap()));
    assert!(
        imported
            .messages()
            .iter()
            .any(|message| message.text() == "imported")
    );
    let copy_name = name(&Some(copy.clone()));
    assert_eq!(
        host.log()[..2],
        [
            format!("before_switch:Resume:{copy_name}"),
            format!("shutdown:Resume:{copy_name}"),
        ]
    );

    // A name taken gets a number.
    runtime.import_from_jsonl(&source, None).await.unwrap();
    let stem = source.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        runtime.session().session_file().unwrap(),
        dir.join(format!("{stem}-1.jsonl"))
    );
    // A file that is not there is reported as such.
    let missing = host.project.join("missing.jsonl");
    assert!(matches!(
        runtime.import_from_jsonl(&missing, None).await,
        Err(AgentSessionError::ImportFileNotFound(path)) if path == missing
    ));
}

#[tokio::test]
async fn a_stored_session_whose_folder_is_gone_needs_another() {
    let host = Host::new();
    // A session recorded in a folder that is no longer there.
    let gone = host._root.path().join("gone");
    let mut stored = SessionManager::create_in(
        gone.to_string_lossy(),
        &default_session_dir(&host.project, &host.agent),
    );
    stored.append_message(SessionMessage::Llm(Message::User(UserMessage::text("old"))));
    let file = stored.session_file().unwrap().to_path_buf();
    drop(stored);

    let runtime = host.runtime(None).await;
    let result = runtime
        .switch_session(&file, SwitchSessionOptions::default())
        .await;
    let Err(AgentSessionError::MissingSessionCwd(issue)) = result else {
        panic!("expected a missing folder");
    };
    assert_eq!(issue.session_file, file);
    assert_eq!(issue.session_cwd, gone.to_string_lossy());
    assert_eq!(issue.fallback_cwd, host.project);
    assert!(
        AgentSessionError::MissingSessionCwd(issue)
            .to_string()
            .starts_with("Stored session working directory does not exist: ")
    );

    // Continuing in another folder works.
    runtime
        .switch_session(
            &file,
            SwitchSessionOptions {
                cwd_override: Some(host.project.to_string_lossy().into_owned()),
                with_session: None,
            },
        )
        .await
        .unwrap();
    let session = runtime.session();
    assert_eq!(
        session.with_session(|tree| tree.cwd().to_string()),
        host.project.to_string_lossy()
    );
    assert_eq!(runtime.cwd(), host.project);
    assert!(
        session
            .messages()
            .iter()
            .any(|message| message.text() == "old")
    );
}

#[tokio::test]
async fn without_a_runtime_a_command_forks_in_place() {
    let host = Host::new();
    let mut paths = ResourcePaths::new(&host.agent, ".maple");
    paths.home_dir = None;
    let mut options = CreateAgentSessionOptions::new("Maple", paths, host.models.clone());
    options.cwd = Some(host.project.clone());
    options.builtin_tools = Some(Vec::new());
    let forked = Arc::new(Mutex::new(Vec::new()));
    let sink = forked.clone();
    options.extensions = vec![extension("forker", move |api| {
        let sink = sink.clone();
        api.register_command("branch", "Fork here", move |args, ctx| {
            let sink = sink.clone();
            async move {
                // No runtime owns the session, so this does nothing, as in Pi.
                ctx.new_session(NewSessionOptions::default()).await?;
                let edited = ctx
                    .fork(
                        &args,
                        ForkOptions {
                            position: ForkPosition::Before,
                            with_session: Some(Box::new(move |ctx| {
                                async move {
                                    sink.lock().unwrap().push(name(&ctx.session_file()));
                                }
                                .boxed()
                            })),
                        },
                    )
                    .await?;
                assert_eq!(edited.as_deref(), Some("second"));
                Ok(())
            }
        });
    })];
    let session = create_agent_session(options).await.unwrap().session;
    host.chat(&session, "first").await;
    host.chat(&session, "second").await;
    let (id, file) = (session.session_id(), session.session_file().unwrap());
    let second = session.with_session(|tree| {
        tree.branch()
            .iter()
            .rev()
            .find(|entry| matches!(&entry.kind, EntryKind::Message { message } if message.text() == "second"))
            .unwrap()
            .id
            .clone()
    });

    session
        .prompt(&format!("/branch {second}"), PromptOptions::default())
        .await
        .unwrap();
    // The same session goes on as the fork, kept in a new file next to the old one.
    assert_ne!(session.session_id(), id);
    let fork_file = session.session_file().unwrap();
    assert_ne!(fork_file, file);
    assert_eq!(fork_file.parent(), file.parent());
    assert_eq!(*forked.lock().unwrap(), [name(&Some(fork_file))]);
}
