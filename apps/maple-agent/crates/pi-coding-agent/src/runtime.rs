//! The session a host is on, replaced as Pi's `AgentSessionRuntime` replaces it: by a
//! new session, a stored one, a fork, or a session file imported.
//!
//! Each replacement asks the extensions first (`session_before_switch` or
//! `session_before_fork`, which can cancel it), stops and shuts down the session it
//! leaves (`session_shutdown`, with why and what replaces it), and builds the next one
//! with the host's [`RuntimeFactory`], whose extensions hear `session_start` with why it
//! started. Commands of a session the runtime owns replace it through the runtime.

use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use async_trait::async_trait;
use futures_util::future::BoxFuture;

use crate::agent_session::{AgentSession, AgentSessionError, SessionCwdIssue, fork_target};
use crate::extensions::{
    CommandContext, ForkPosition, SessionBeforeFork, SessionBeforeSwitch, SessionShutdown,
    SessionShutdownReason, SessionStart, SessionStartReason, SessionSwitchReason,
};
use crate::sdk::CreateAgentSessionResult;
use crate::session::SessionManager;
use crate::tools::resolve_to_cwd;

/// What a runtime builds a session for, as Pi hands its runtime factory a target: the
/// working directory, the session to continue, and what its extensions hear as it
/// starts.
pub struct RuntimeTarget {
    pub cwd: PathBuf,
    pub session: SessionManager,
    pub session_start: SessionStart,
}

/// Builds each session a runtime moves to, as Pi's `CreateAgentSessionRuntimeFactory`:
/// typically [`create_agent_session`](crate::create_agent_session) with the host's
/// fixed options and the target's. Problems the result reports, such as settings that
/// did not load, are the factory's to show.
#[async_trait]
pub trait RuntimeFactory: Send + Sync {
    async fn create(
        &self,
        target: RuntimeTarget,
    ) -> Result<CreateAgentSessionResult, AgentSessionError>;
}

#[async_trait]
impl<F, Fut> RuntimeFactory for F
where
    F: Fn(RuntimeTarget) -> Fut + Send + Sync,
    Fut: Future<Output = Result<CreateAgentSessionResult, AgentSessionError>> + Send,
{
    async fn create(
        &self,
        target: RuntimeTarget,
    ) -> Result<CreateAgentSessionResult, AgentSessionError> {
        self(target).await
    }
}

/// Run with the command context of the session that replaced the one before, as Pi's
/// `withSession`: where to go on once a command has replaced its own session.
pub type WithSession = Box<dyn FnOnce(CommandContext) -> BoxFuture<'static, ()> + Send>;

/// Entries a new session gets before it goes on, as Pi's `setup`.
pub type SessionSetup = Box<dyn FnOnce(&mut SessionManager) + Send>;

/// Called with each session that replaces the one before, as Pi's `setRebindSession`:
/// where a host subscribes to the new session.
pub type RebindSession = Arc<dyn Fn(AgentSession) -> BoxFuture<'static, ()> + Send + Sync>;

/// How [`AgentSessionRuntime::new_session`] starts one, as Pi's `newSession` options.
#[derive(Default)]
pub struct NewSessionOptions {
    /// The session file the new session names as its parent.
    pub parent_session: Option<String>,
    pub setup: Option<SessionSetup>,
    pub with_session: Option<WithSession>,
}

/// How [`AgentSessionRuntime::fork`] forks, as Pi's `fork` options.
#[derive(Default)]
pub struct ForkOptions {
    pub position: ForkPosition,
    pub with_session: Option<WithSession>,
}

/// How [`AgentSessionRuntime::switch_session`] switches, as Pi's `switchSession` options.
#[derive(Default)]
pub struct SwitchSessionOptions {
    /// The working directory to continue in, instead of the one the session recorded.
    pub cwd_override: Option<String>,
    pub with_session: Option<WithSession>,
}

/// What a session's commands replace it with: its runtime.
#[async_trait]
pub(crate) trait SessionActions: Send + Sync {
    async fn new_session(&self, options: NewSessionOptions) -> Result<(), AgentSessionError>;
    async fn switch_session(
        &self,
        path: &Path,
        options: SwitchSessionOptions,
    ) -> Result<(), AgentSessionError>;
    async fn fork(
        &self,
        entry_id: &str,
        options: ForkOptions,
    ) -> Result<Option<String>, AgentSessionError>;
}

/// The session a host is on and how to replace it, as Pi's `AgentSessionRuntime`.
///
/// Replacing the session shuts the one before down and drops it; a host keeps no
/// session of its own but takes [`Self::session`] again, or is handed it by
/// [`Self::set_rebind_session`].
#[derive(Clone)]
pub struct AgentSessionRuntime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    factory: Arc<dyn RuntimeFactory>,
    current: Mutex<Current>,
    rebind: Mutex<Option<RebindSession>>,
    /// Held while the session is being replaced; another replacement meanwhile, such as
    /// one an extension starts as it hears of this one, is refused as busy.
    replacing: tokio::sync::Mutex<()>,
    weak: Weak<RuntimeInner>,
}

struct Current {
    session: AgentSession,
    cwd: PathBuf,
    model_fallback_message: Option<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The file `session` is kept in, when its store keeps one.
fn file_of(session: &SessionManager) -> Option<PathBuf> {
    session.session_file().map(Path::to_path_buf)
}

fn io_error(error: io::Error) -> AgentSessionError {
    AgentSessionError::Failed(error.to_string())
}

/// `path` as Pi resolves one: `~` is the home folder, and a relative path is relative
/// to the process's working directory.
fn resolve(path: &Path) -> Result<PathBuf, AgentSessionError> {
    let cwd = std::env::current_dir().map_err(io_error)?;
    Ok(resolve_to_cwd(&path.to_string_lossy(), &cwd))
}

/// Refuse a stored session whose working directory is gone, as Pi's
/// `assertSessionCwdExists`.
fn check_session_cwd(
    session: &SessionManager,
    fallback_cwd: &Path,
) -> Result<(), AgentSessionError> {
    let Some(file) = session.session_file() else {
        return Ok(());
    };
    let cwd = session.cwd();
    if cwd.is_empty() || Path::new(cwd).exists() {
        return Ok(());
    }
    Err(AgentSessionError::MissingSessionCwd(SessionCwdIssue {
        session_file: file.to_path_buf(),
        session_cwd: cwd.to_string(),
        fallback_cwd: fallback_cwd.to_path_buf(),
    }))
}

/// Build the first session with `factory`, and the runtime that replaces it with
/// sessions `factory` builds, as Pi's `createAgentSessionRuntime`.
pub async fn create_agent_session_runtime(
    factory: impl RuntimeFactory + 'static,
    target: RuntimeTarget,
) -> Result<AgentSessionRuntime, AgentSessionError> {
    check_session_cwd(&target.session, &target.cwd)?;
    let factory: Arc<dyn RuntimeFactory> = Arc::new(factory);
    let cwd = target.cwd.clone();
    let result = factory.create(target).await?;
    let inner = Arc::new_cyclic(|weak| RuntimeInner {
        factory,
        current: Mutex::new(Current {
            session: result.session,
            cwd,
            model_fallback_message: result.model_fallback_message,
        }),
        rebind: Mutex::new(None),
        replacing: tokio::sync::Mutex::new(()),
        weak: weak.clone(),
    });
    inner.bind_current();
    Ok(AgentSessionRuntime { inner })
}

impl AgentSessionRuntime {
    /// The session the host is on.
    pub fn session(&self) -> AgentSession {
        self.inner.session()
    }

    /// The working directory of the session the host is on.
    pub fn cwd(&self) -> PathBuf {
        lock(&self.inner.current).cwd.clone()
    }

    /// Why the current session did not start with the model it recorded, as Pi's
    /// `modelFallbackMessage`.
    pub fn model_fallback_message(&self) -> Option<String> {
        lock(&self.inner.current).model_fallback_message.clone()
    }

    /// Call `rebind` with each session that replaces the one before, before any
    /// `with_session`, as Pi's `setRebindSession`.
    pub fn set_rebind_session(
        &self,
        rebind: impl Fn(AgentSession) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    ) {
        *lock(&self.inner.rebind) = Some(Arc::new(rebind));
    }

    /// Go on with the session stored in `path`, as Pi's `switchSession`. Extensions can
    /// cancel it with `session_before_switch`, which fails with `Cancelled`.
    pub async fn switch_session(
        &self,
        path: &Path,
        options: SwitchSessionOptions,
    ) -> Result<(), AgentSessionError> {
        self.inner.switch_session(path, options).await
    }

    /// Go on with a new session, as Pi's `newSession`: kept next to the current one's
    /// file, or in memory when the current one is. Extensions can cancel it with
    /// `session_before_switch`.
    pub async fn new_session(&self, options: NewSessionOptions) -> Result<(), AgentSessionError> {
        self.inner.new_session(options).await
    }

    /// Go on with a new session holding only the path to `entry_id`, as Pi's `fork`, and
    /// give back the text of a user message forked before. Extensions can cancel it with
    /// `session_before_fork`.
    pub async fn fork(
        &self,
        entry_id: &str,
        options: ForkOptions,
    ) -> Result<Option<String>, AgentSessionError> {
        self.inner.fork(entry_id, options).await
    }

    /// Copy the session file at `path` into the current session's folder, unless it is
    /// there already, and go on with it, as Pi's `importFromJsonl`. A name taken there
    /// gets `-1`, `-2` and so on.
    pub async fn import_from_jsonl(
        &self,
        path: &Path,
        cwd_override: Option<&str>,
    ) -> Result<(), AgentSessionError> {
        self.inner.import_from_jsonl(path, cwd_override).await
    }

    /// Shut the current session down because the host is done with it, as Pi's
    /// `dispose`.
    pub async fn dispose(&self) {
        self.session().shutdown().await;
    }
}

impl RuntimeInner {
    fn session(&self) -> AgentSession {
        lock(&self.current).session.clone()
    }

    fn current_parts(&self) -> (AgentSession, PathBuf) {
        let current = lock(&self.current);
        (current.session.clone(), current.cwd.clone())
    }

    fn bind_current(&self) {
        let actions: Weak<dyn SessionActions> = self.weak.clone();
        self.session().bind_actions(actions);
    }

    fn begin(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, AgentSessionError> {
        self.replacing
            .try_lock()
            .map_err(|_| AgentSessionError::Busy)
    }

    async fn before_switch(
        &self,
        session: &AgentSession,
        reason: SessionSwitchReason,
        target_session_file: Option<PathBuf>,
    ) -> Result<(), AgentSessionError> {
        let answer = session
            .before_switch(SessionBeforeSwitch {
                reason,
                target_session_file,
            })
            .await;
        if answer.cancel {
            return Err(AgentSessionError::Cancelled);
        }
        Ok(())
    }

    /// Stop the session being left, so its last turn is recorded, and shut it down,
    /// naming the file of the session that replaces it. Its commands no longer replace
    /// the runtime's session, as Pi's stale contexts no longer act.
    async fn teardown(
        &self,
        session: &AgentSession,
        reason: SessionShutdownReason,
        target_session_file: Option<PathBuf>,
    ) {
        session
            .shutdown_with(SessionShutdown {
                reason,
                target_session_file,
            })
            .await;
        session.unbind_actions();
    }

    /// Build the next session and make it the current one.
    async fn replace(&self, target: RuntimeTarget) -> Result<AgentSession, AgentSessionError> {
        let cwd = target.cwd.clone();
        let result = self.factory.create(target).await?;
        let session = result.session.clone();
        *lock(&self.current) = Current {
            session: result.session,
            cwd,
            model_fallback_message: result.model_fallback_message,
        };
        self.bind_current();
        Ok(session)
    }

    async fn finish(&self, session: AgentSession, with_session: Option<WithSession>) {
        let rebind = lock(&self.rebind).clone();
        if let Some(rebind) = rebind {
            rebind(session.clone()).await;
        }
        if let Some(with_session) = with_session {
            with_session(session.command_context()).await;
        }
    }

    async fn switch_session(
        &self,
        path: &Path,
        options: SwitchSessionOptions,
    ) -> Result<(), AgentSessionError> {
        let replacing = self.begin()?;
        let path = resolve(path)?;
        let (session, cwd) = self.current_parts();
        self.before_switch(&session, SessionSwitchReason::Resume, Some(path.clone()))
            .await?;
        let previous_session_file = session.session_file();
        let manager =
            SessionManager::open_file(&path, options.cwd_override.as_deref()).map_err(io_error)?;
        check_session_cwd(&manager, &cwd)?;
        self.teardown(&session, SessionShutdownReason::Resume, file_of(&manager))
            .await;
        let next = self
            .replace(RuntimeTarget {
                cwd: PathBuf::from(manager.cwd()),
                session: manager,
                session_start: SessionStart {
                    reason: SessionStartReason::Resume,
                    previous_session_file,
                },
            })
            .await?;
        drop(replacing);
        self.finish(next, options.with_session).await;
        Ok(())
    }

    async fn new_session(&self, options: NewSessionOptions) -> Result<(), AgentSessionError> {
        let replacing = self.begin()?;
        let (session, cwd) = self.current_parts();
        self.before_switch(&session, SessionSwitchReason::New, None)
            .await?;
        let previous_session_file = session.session_file();
        let folder = cwd.to_string_lossy().into_owned();
        let mut manager = match previous_session_file.as_deref().and_then(Path::parent) {
            Some(dir) => SessionManager::create_in(folder, dir),
            None => SessionManager::in_memory(folder),
        };
        if let Some(parent) = options.parent_session {
            manager = manager.with_parent_session(parent);
        }
        self.teardown(&session, SessionShutdownReason::New, file_of(&manager))
            .await;
        let next = self
            .replace(RuntimeTarget {
                cwd,
                session: manager,
                session_start: SessionStart {
                    reason: SessionStartReason::New,
                    previous_session_file,
                },
            })
            .await?;
        if let Some(setup) = options.setup {
            next.setup_session(setup);
        }
        drop(replacing);
        self.finish(next, options.with_session).await;
        Ok(())
    }

    async fn fork(
        &self,
        entry_id: &str,
        options: ForkOptions,
    ) -> Result<Option<String>, AgentSessionError> {
        let replacing = self.begin()?;
        let (session, _) = self.current_parts();
        let answer = session
            .before_fork(SessionBeforeFork {
                entry_id: entry_id.to_string(),
                position: options.position,
            })
            .await;
        if answer.cancel {
            return Err(AgentSessionError::Cancelled);
        }
        let (leaf, edited_text) =
            session.with_session(|tree| fork_target(tree, entry_id, options.position))?;
        let previous_session_file = session.session_file();
        // A fork is kept next to the session's file, as in Pi, or in memory with it.
        let dir = previous_session_file
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        if let (Some(file), Some(_)) = (&previous_session_file, &leaf)
            && !file.exists()
        {
            return Err(AgentSessionError::Failed(
                "This session has not been saved yet. Send a message before cloning or forking it."
                    .into(),
            ));
        }
        let manager =
            session.with_session(|tree| tree.branched(leaf.as_deref(), dir.as_deref()))?;
        self.teardown(&session, SessionShutdownReason::Fork, file_of(&manager))
            .await;
        let next = self
            .replace(RuntimeTarget {
                cwd: PathBuf::from(manager.cwd()),
                session: manager,
                session_start: SessionStart {
                    reason: SessionStartReason::Fork,
                    previous_session_file,
                },
            })
            .await?;
        drop(replacing);
        self.finish(next, options.with_session).await;
        Ok(edited_text)
    }

    async fn import_from_jsonl(
        &self,
        path: &Path,
        cwd_override: Option<&str>,
    ) -> Result<(), AgentSessionError> {
        let replacing = self.begin()?;
        let source = resolve(path)?;
        if !source.exists() {
            return Err(AgentSessionError::ImportFileNotFound(source));
        }
        let (session, cwd) = self.current_parts();
        let previous_session_file = session.session_file();
        let Some(dir) = previous_session_file
            .as_deref()
            .and_then(Path::parent)
            .map(resolve)
            .transpose()?
        else {
            return Err(AgentSessionError::Failed(
                "This session is not kept in a file, so there is no folder to import into".into(),
            ));
        };
        fs::create_dir_all(&dir).map_err(io_error)?;
        let name = source.file_name().unwrap_or_default();
        let mut destination = dir.join(name);
        let already_stored = destination == source;
        if !already_stored {
            let stem = Path::new(name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let extension = Path::new(name)
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy()))
                .unwrap_or_default();
            let mut suffix = 1;
            while destination.exists() {
                destination = dir.join(format!("{stem}-{suffix}{extension}"));
                suffix += 1;
            }
        }
        self.before_switch(
            &session,
            SessionSwitchReason::Resume,
            Some(destination.clone()),
        )
        .await?;
        if !already_stored {
            let mut from = fs::File::open(&source).map_err(io_error)?;
            let mut to = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(io_error)?;
            io::copy(&mut from, &mut to).map_err(io_error)?;
        }
        let manager = SessionManager::open_file(&destination, cwd_override).map_err(io_error)?;
        check_session_cwd(&manager, &cwd)?;
        self.teardown(&session, SessionShutdownReason::Resume, file_of(&manager))
            .await;
        let next = self
            .replace(RuntimeTarget {
                cwd: PathBuf::from(manager.cwd()),
                session: manager,
                session_start: SessionStart {
                    reason: SessionStartReason::Resume,
                    previous_session_file,
                },
            })
            .await?;
        drop(replacing);
        self.finish(next, None).await;
        Ok(())
    }
}

#[async_trait]
impl SessionActions for RuntimeInner {
    async fn new_session(&self, options: NewSessionOptions) -> Result<(), AgentSessionError> {
        RuntimeInner::new_session(self, options).await
    }

    async fn switch_session(
        &self,
        path: &Path,
        options: SwitchSessionOptions,
    ) -> Result<(), AgentSessionError> {
        RuntimeInner::switch_session(self, path, options).await
    }

    async fn fork(
        &self,
        entry_id: &str,
        options: ForkOptions,
    ) -> Result<Option<String>, AgentSessionError> {
        RuntimeInner::fork(self, entry_id, options).await
    }
}
