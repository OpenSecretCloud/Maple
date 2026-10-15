//! OS-local endpoints and startup arbitration. No TCP listener or credentials.

use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
};
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Connection = Box<dyn Stream>;

#[derive(Clone)]
pub struct Endpoint {
    pub profile: String,
    directory: PathBuf,
    address: PathBuf,
}

impl Endpoint {
    pub fn new(api_url: &str) -> Result<Self, String> {
        let config = crate::backend::app_config_root();
        let data = crate::backend::local::host_data_root();
        std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        // The storage owner is independent of backend settings. A client
        // pointing the same files at another backend must fail the handshake,
        // not create a second writer to those files.
        let storage = serde_json::to_vec(&(
            std::fs::canonicalize(config).map_err(|e| e.to_string())?,
            std::fs::canonicalize(data).map_err(|e| e.to_string())?,
            crate::profile::DATA_NAMESPACE,
            os_user_identity()?,
        ))
        .map_err(|e| e.to_string())?;
        let namespace = format!("{:x}", Sha256::digest(&storage));
        let identity = serde_json::to_vec(&(
            namespace.as_str(),
            (
                cfg!(feature = "desktop"),
                cfg!(feature = "acp"),
                cfg!(feature = "proxy"),
            ),
            api_url,
            crate::billing::configured_billing_api_url(),
            crate::backend::local::configured_client_id(),
            format!(
                "{:?}",
                maple_agent::open_secret_config::configured_pcr0_environment()?
            ),
        ))
        .map_err(|e| e.to_string())?;
        let profile = format!("{:x}", Sha256::digest(identity));
        #[cfg(unix)]
        let directory = {
            // Short even when XDG roots are long (macOS sun_path is 104 bytes).
            let uid = unsafe { libc::geteuid() };
            let parent = PathBuf::from(format!("/tmp/maple-agent-{uid}"));
            private_directory(&parent)?;
            parent.join(&namespace[..24])
        };
        #[cfg(windows)]
        let directory = crate::backend::local::host_data_root().join("host");
        private_directory(&directory)?;
        #[cfg(unix)]
        let address = directory.join("ipc.sock");
        #[cfg(windows)]
        let address = PathBuf::from(format!(r"\\.\pipe\maple-agent-{namespace}"));
        Ok(Self {
            profile,
            directory,
            address,
        })
    }

    pub fn lock(&self, name: &str) -> Result<File, String> {
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        options
            .open(self.directory.join(name))
            .map_err(|e| format!("Cannot open host lock: {e}"))
    }

    fn address(&self) -> &std::path::Path {
        &self.address
    }

    pub async fn connect(&self) -> Result<Connection, String> {
        #[cfg(unix)]
        {
            tokio::net::UnixStream::connect(self.address())
                .await
                .map(|s| Box::new(s) as Connection)
                .map_err(|e| e.to_string())
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let stream = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(self.address())
                .map_err(|e| e.to_string())?;
            verify_pipe_owner(stream.as_raw_handle())?;
            Ok(Box::new(stream))
        }
    }

    pub fn bind(&self) -> Result<Listener, String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, PermissionsExt};
            let path = self.address().to_path_buf();
            if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                if !metadata.file_type().is_socket() {
                    return Err("Host endpoint is not a socket".into());
                }
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            let listener = tokio::net::UnixListener::bind(&path).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
            Ok(Listener {
                inner: listener,
                path,
            })
        }
        #[cfg(windows)]
        {
            Ok(Listener {
                inner: make_pipe(self.address(), true)?,
                path: self.address().to_path_buf(),
            })
        }
    }
}

fn private_directory(path: &std::path::Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("Cannot create host directory: {e}")),
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Unsafe host directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err("Host directory must be private to this OS user".into());
        }
    }
    Ok(())
}

pub struct Listener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    inner: tokio::net::windows::named_pipe::NamedPipeServer,
    path: PathBuf,
}

impl Listener {
    pub async fn accept(&mut self) -> Result<Connection, String> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await.map_err(|e| e.to_string())?;
            let credentials = stream.peer_cred().map_err(|e| e.to_string())?;
            if credentials.uid() != unsafe { libc::geteuid() } {
                return Err("Host peer belongs to another OS user".into());
            }
            Ok(Box::new(stream))
        }
        #[cfg(windows)]
        {
            self.inner.connect().await.map_err(|e| e.to_string())?;
            let next = make_pipe(&self.path, false)?;
            Ok(Box::new(std::mem::replace(&mut self.inner, next)))
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
fn make_pipe(
    path: &std::path::Path,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
    };
    // Owner rights and SYSTEM only. Named-pipe defaults also grant Everyone
    // read access, so the default DACL is not sufficient for a control endpoint.
    let descriptor: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GA;;;SY)\0".encode_utf16().collect();
    let mut security = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            1,
            &mut security,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security,
        bInheritHandle: 0,
    };
    let result = unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(path, &attributes as *const _ as *mut _)
    };
    unsafe {
        LocalFree(security);
    }
    result.map_err(|e| e.to_string())
}

#[cfg(windows)]
fn verify_pipe_owner(pipe: windows_sys::Win32::Foundation::HANDLE) -> Result<(), String> {
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
            EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // Verify the server as well as restricting clients: an unrelated OS user
    // must not pre-create our predictable pipe name and receive sign-in data.
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
        if size == 0 {
            return Err("Cannot read local user identity".into());
        }
        let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut owner = null_mut();
        let mut descriptor = null_mut();
        let status = GetSecurityInfo(
            pipe,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(format!("Cannot verify Maple host ownership ({status})"));
        }
        let matches = !owner.is_null() && EqualSid(owner, user.User.Sid) != 0;
        LocalFree(descriptor);
        if matches {
            Ok(())
        } else {
            Err("Maple host belongs to another OS user".into())
        }
    }
}

#[cfg(unix)]
fn os_user_identity() -> Result<String, String> {
    Ok(unsafe { libc::geteuid() }.to_string())
}

#[cfg(windows)]
fn os_user_identity() -> Result<String, String> {
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
        if size == 0 {
            return Err("Cannot read local user identity".into());
        }
        let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut text = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(sid)
    }
}
