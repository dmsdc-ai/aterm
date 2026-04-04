use std::ffi::{CStr, CString};
#[cfg(unix)]
use std::os::fd::IntoRawFd;
use std::os::raw::{c_char, c_int};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

fn tsnet_link_anchor() -> usize {
    std::mem::size_of::<tsnet::Network>()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TailscalePhase {
    Idle,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleStatusSnapshot {
    pub phase: TailscalePhase,
    pub hostname: String,
    pub control_url: Option<String>,
    pub state_dir: String,
    pub ips: Vec<String>,
    pub auth_url: Option<String>,
    pub auth_key_present: bool,
    pub last_error: Option<String>,
    pub last_log_line: Option<String>,
    pub started_at_epoch_ms: Option<u64>,
    pub updated_at_epoch_ms: u64,
}

#[derive(Debug, Clone)]
struct TailscaleConfig {
    hostname: String,
    control_url: Option<String>,
    auth_key: Option<String>,
    state_dir: PathBuf,
}

impl TailscaleConfig {
    fn from_inputs(
        hostname: Option<&str>,
        control_url: Option<&str>,
        auth_key: Option<&str>,
    ) -> Result<Self, String> {
        let hostname = hostname
            .filter(|value| !value.trim().is_empty())
            .map(sanitize_hostname)
            .or_else(|| {
                std::env::var("ATERM_TAILSCALE_HOSTNAME")
                    .ok()
                    .map(|value| sanitize_hostname(&value))
            })
            .unwrap_or_else(default_hostname);

        let control_url = control_url
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .or_else(|| {
                std::env::var("ATERM_TAILSCALE_CONTROL_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });

        let auth_key = auth_key
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .or_else(|| {
                std::env::var("ATERM_TAILSCALE_AUTHKEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });

        let state_dir = std::env::var("ATERM_TAILSCALE_STATE_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_state_dir);

        std::fs::create_dir_all(&state_dir).map_err(|error| error.to_string())?;

        Ok(Self {
            hostname,
            control_url,
            auth_key,
            state_dir,
        })
    }
}

#[derive(Debug)]
struct RawServer {
    handle: c_int,
}

impl RawServer {
    fn new(
        config: &TailscaleConfig,
        state: Arc<Mutex<TailscaleState>>,
        generation: u64,
    ) -> Result<Self, String> {
        let _ = tsnet_link_anchor();

        let server = Self {
            handle: unsafe { tailscale_new() },
        };
        server.attach_log_pipe(state, generation)?;

        let state_dir = config
            .state_dir
            .to_str()
            .ok_or_else(|| "tailscale state dir contains invalid UTF-8".to_string())?;
        server.set_string(state_dir, tailscale_set_dir)?;
        server.set_string(&config.hostname, tailscale_set_hostname)?;

        if let Some(control_url) = config.control_url.as_deref() {
            server.set_string(control_url, tailscale_set_control_url)?;
        }
        if let Some(auth_key) = config.auth_key.as_deref() {
            server.set_string(auth_key, tailscale_set_authkey)?;
        }

        Ok(server)
    }

    fn start(&self) -> Result<(), String> {
        self.check(unsafe { tailscale_start(self.handle) })
    }

    fn up(&self) -> Result<(), String> {
        self.check(unsafe { tailscale_up(self.handle) })
    }

    fn set_string(
        &self,
        value: &str,
        setter: unsafe extern "C" fn(c_int, *const c_char) -> c_int,
    ) -> Result<(), String> {
        let c_string = CString::new(value).map_err(|error| error.to_string())?;
        self.check(unsafe { setter(self.handle, c_string.as_ptr()) })
    }

    fn check(&self, code: c_int) -> Result<(), String> {
        match code {
            0 => Ok(()),
            -1 => Err(self.last_error()),
            errno => Err(std::io::Error::from_raw_os_error(errno).to_string()),
        }
    }

    fn last_error(&self) -> String {
        let mut buffer = vec![0_i8; 512];
        let code = unsafe { tailscale_errmsg(self.handle, buffer.as_mut_ptr(), buffer.len()) };
        if code != 0 {
            return format!("tailscale error code {code}");
        }

        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string()
    }

    #[cfg(unix)]
    fn attach_log_pipe(
        &self,
        state: Arc<Mutex<TailscaleState>>,
        generation: u64,
    ) -> Result<(), String> {
        let (reader, writer) = UnixStream::pair().map_err(|error| error.to_string())?;
        let log_fd = writer.into_raw_fd();
        self.check(unsafe { tailscale_set_logfd(self.handle, log_fd) })?;

        thread::Builder::new()
            .name("aterm-tailscale-log".to_string())
            .spawn(move || read_tailscale_logs(reader, state, generation))
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    #[cfg(not(unix))]
    fn attach_log_pipe(
        &self,
        _state: Arc<Mutex<TailscaleState>>,
        _generation: u64,
    ) -> Result<(), String> {
        Ok(())
    }
}

impl Drop for RawServer {
    fn drop(&mut self) {
        let _ = unsafe { tailscale_close(self.handle) };
    }
}

#[derive(Debug)]
struct TailscaleState {
    generation: u64,
    phase: TailscalePhase,
    hostname: String,
    control_url: Option<String>,
    state_dir: String,
    ips: Vec<String>,
    auth_url: Option<String>,
    auth_key_present: bool,
    last_error: Option<String>,
    last_log_line: Option<String>,
    started_at_epoch_ms: Option<u64>,
    updated_at_epoch_ms: u64,
    server: Option<RawServer>,
}

impl Default for TailscaleState {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: TailscalePhase::Idle,
            hostname: default_hostname(),
            control_url: None,
            state_dir: default_state_dir().display().to_string(),
            ips: Vec::new(),
            auth_url: None,
            auth_key_present: false,
            last_error: None,
            last_log_line: None,
            started_at_epoch_ms: None,
            updated_at_epoch_ms: now_epoch_ms(),
            server: None,
        }
    }
}

impl TailscaleState {
    fn snapshot(&self) -> TailscaleStatusSnapshot {
        TailscaleStatusSnapshot {
            phase: self.phase.clone(),
            hostname: self.hostname.clone(),
            control_url: self.control_url.clone(),
            state_dir: self.state_dir.clone(),
            ips: self.ips.clone(),
            auth_url: self.auth_url.clone(),
            auth_key_present: self.auth_key_present,
            last_error: self.last_error.clone(),
            last_log_line: self.last_log_line.clone(),
            started_at_epoch_ms: self.started_at_epoch_ms,
            updated_at_epoch_ms: self.updated_at_epoch_ms,
        }
    }
}

pub struct TailscaleManager {
    state: Arc<Mutex<TailscaleState>>,
}

impl TailscaleManager {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TailscaleState::default())),
        }
    }

    pub fn connect(
        &self,
        hostname: Option<&str>,
        control_url: Option<&str>,
        auth_key: Option<&str>,
    ) -> Result<(), String> {
        let config = TailscaleConfig::from_inputs(hostname, control_url, auth_key)?;
        let generation = {
            let mut state = self.state.lock().map_err(|error| error.to_string())?;
            if matches!(
                state.phase,
                TailscalePhase::Starting | TailscalePhase::Running
            ) {
                return Ok(());
            }

            state.generation += 1;
            state.phase = TailscalePhase::Starting;
            state.hostname = config.hostname.clone();
            state.control_url = config.control_url.clone();
            state.state_dir = config.state_dir.display().to_string();
            state.ips.clear();
            state.auth_url = None;
            state.auth_key_present = config.auth_key.is_some();
            state.last_error = None;
            state.last_log_line = None;
            state.started_at_epoch_ms = Some(now_epoch_ms());
            state.updated_at_epoch_ms = now_epoch_ms();
            state.generation
        };

        let state = Arc::clone(&self.state);
        thread::Builder::new()
            .name("aterm-tailscale-start".to_string())
            .spawn(move || {
                if let Err(error) = run_connect(state.clone(), generation, config) {
                    if let Ok(mut guard) = state.lock() {
                        if guard.generation == generation {
                            guard.phase = TailscalePhase::Error;
                            guard.last_error = Some(error);
                            guard.server = None;
                            guard.updated_at_epoch_ms = now_epoch_ms();
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    pub fn shutdown(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.generation += 1;
            state.phase = TailscalePhase::Idle;
            state.ips.clear();
            state.auth_url = None;
            state.last_error = None;
            state.last_log_line = None;
            state.started_at_epoch_ms = None;
            state.updated_at_epoch_ms = now_epoch_ms();
            state.server = None;
        }
    }

    pub fn status(&self) -> TailscaleStatusSnapshot {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .unwrap_or_else(|_| TailscaleState::default().snapshot())
    }
}

pub fn global_manager() -> &'static TailscaleManager {
    static INSTANCE: OnceLock<TailscaleManager> = OnceLock::new();
    INSTANCE.get_or_init(TailscaleManager::new)
}

fn run_connect(
    state: Arc<Mutex<TailscaleState>>,
    generation: u64,
    config: TailscaleConfig,
) -> Result<(), String> {
    let server = RawServer::new(&config, Arc::clone(&state), generation)?;
    server.start()?;
    let handle = server.handle;

    {
        let mut guard = state.lock().map_err(|error| error.to_string())?;
        if guard.generation != generation {
            return Ok(());
        }
        guard.server = Some(server);
        guard.updated_at_epoch_ms = now_epoch_ms();
    }

    {
        let guard = state.lock().map_err(|error| error.to_string())?;
        if guard.generation != generation || guard.server.is_none() {
            return Ok(());
        }
    }

    let server = RawServer { handle };
    let up_result = server.up();
    std::mem::forget(server);
    up_result?;

    if let Ok(mut guard) = state.lock() {
        if guard.generation == generation {
            guard.phase = TailscalePhase::Running;
            guard.auth_url = None;
            guard.updated_at_epoch_ms = now_epoch_ms();
        }
    }
    Ok(())
}

#[cfg(unix)]
fn read_tailscale_logs(reader: UnixStream, state: Arc<Mutex<TailscaleState>>, generation: u64) {
    use std::io::{BufRead, BufReader};

    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(mut guard) = state.lock() {
                    if guard.generation != generation {
                        break;
                    }
                    guard.last_log_line = Some(trimmed.clone());
                    if let Some(url) = extract_auth_url(&trimmed) {
                        guard.auth_url = Some(url);
                    }
                    guard.updated_at_epoch_ms = now_epoch_ms();
                }
            }
            Err(_) => break,
        }
    }
}

fn extract_auth_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|token| token.starts_with("https://") && token.contains("tailscale.com"))
        .map(|token| {
            token
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | '(' | ')' | '[' | ']'))
                .to_string()
        })
}

fn default_state_dir() -> PathBuf {
    crate::session::data_root().join("tailscale")
}

fn default_hostname() -> String {
    let raw = std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("COMPUTERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(read_hostname_command)
        .unwrap_or_else(|| "aterm".to_string());

    let sanitized = sanitize_hostname(&raw);
    if sanitized.starts_with("aterm-") {
        sanitized
    } else {
        format!("aterm-{sanitized}")
    }
}

fn read_hostname_command() -> Option<String> {
    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hostname.is_empty() {
        None
    } else {
        Some(hostname)
    }
}

fn sanitize_hostname(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            normalized.push('-');
        }
    }

    let collapsed = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if collapsed.is_empty() {
        "aterm".to_string()
    } else {
        collapsed
    }
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

extern "C" {
    fn tailscale_new() -> c_int;
    fn tailscale_start(sd: c_int) -> c_int;
    fn tailscale_up(sd: c_int) -> c_int;
    fn tailscale_close(sd: c_int) -> c_int;
    fn tailscale_set_dir(sd: c_int, dir: *const c_char) -> c_int;
    fn tailscale_set_hostname(sd: c_int, hostname: *const c_char) -> c_int;
    fn tailscale_set_authkey(sd: c_int, authkey: *const c_char) -> c_int;
    fn tailscale_set_control_url(sd: c_int, control_url: *const c_char) -> c_int;
    fn tailscale_set_logfd(sd: c_int, fd: c_int) -> c_int;
    fn tailscale_errmsg(sd: c_int, buf: *mut c_char, buflen: usize) -> c_int;
}
