use crate::ipc::{Connection, ConnectionTmpl};
use hbb_common::{anyhow, bail, log, ResultType};
use parity_tokio_ipc::SecurityAttributes;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::{
    fs,
    path::{Path, PathBuf},
};
use windows::Win32::{Foundation::HANDLE, System::Pipes::GetNamedPipeClientProcessId};

#[inline]
pub(crate) fn should_allow_everyone_create_on_windows(postfix: &str) -> bool {
    postfix.is_empty() || hbb_common::config::is_service_ipc_postfix(postfix)
}

#[inline]
pub(crate) fn portable_service_listener_security_attributes() -> io::Result<SecurityAttributes> {
    let user_sid = crate::platform::windows::current_process_user_sid_string().map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("failed to resolve current process SID: {}", err),
        )
    })?;
    debug_assert!(
        user_sid.starts_with("S-1-")
            && user_sid
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-'),
        "current_process_user_sid_string returned a non-SDDL SID: {}",
        user_sid
    );
    // SDDL:
    // - `D:P`                => protected DACL (no inherited ACEs)
    // - `(A;;GA;;;SY)`       => allow GENERIC_ALL to LocalSystem
    // - `(A;;GA;;;{user_sid})` => allow GENERIC_ALL to current process user SID
    // References:
    // - Security Descriptor String Format: https://learn.microsoft.com/en-us/windows/win32/secauthz/security-descriptor-string-format
    // - ACE strings in SDDL: https://learn.microsoft.com/en-us/windows/win32/secauthz/ace-strings
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})");
    SecurityAttributes::from_sddl(&sddl).map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!(
                "failed to build portable service listener security attributes from SDDL '{}': {}",
                sddl, err
            ),
        )
    })
}


#[inline]
fn windows_portable_service_ipc_allows_logon_helper_executable(
    _peer_exe: &Path,
    postfix: &str,
) -> bool {
    if postfix != "_portable_service" {
        return false;
    }
    false
}

#[inline]
pub(crate) fn is_allowed_windows_session_scoped_peer(
    client_is_system: bool,
    client_session_id: Option<u32>,
    expected_session_id: Option<u32>,
) -> bool {
    client_is_system
        || matches!(
            (client_session_id, expected_session_id),
            (Some(client), Some(expected)) if client == expected
        )
}

#[inline]
fn is_allowed_windows_portable_service_peer(
    client_is_system: Option<bool>,
    _client_session_id: Option<u32>,
    _expected_session_id: Option<u32>,
) -> bool {
    // Portable-service listener DACL includes SYSTEM and current-process SID.
    // In the portable-service path, current process is expected to run as SYSTEM,
    // and the higher-layer peer policy stays SYSTEM-only.
    matches!(client_is_system, Some(true))
}










#[inline]
fn current_exe_canonical_path() -> ResultType<PathBuf> {
    let current = std::env::current_exe()
        .map_err(|err| anyhow::anyhow!("Failed to resolve current executable path: {}", err))?;
    fs::canonicalize(&current).map_err(|err| {
        anyhow::anyhow!(
            "Failed to canonicalize current executable path '{}': {}",
            current.display(),
            err
        )
        .into()
    })
}



#[inline]
fn peer_exe_canonical_path_by_pid(peer_pid: u32) -> ResultType<PathBuf> {
    let path = crate::platform::windows::get_process_executable_path(peer_pid)?;
    fs::canonicalize(&path).map_err(|err| {
        anyhow::anyhow!(
            "Failed to canonicalize peer executable path '{}': {}",
            path.display(),
            err
        )
        .into()
    })
}

#[inline]
pub(crate) fn executable_paths_match(left: &Path, right: &Path) -> bool {
    {
        // Callers pass paths resolved through fs::canonicalize() first, so NT
        // namespace paths and 8.3 short names are expected to be resolved before
        // this check. Keep this normalization limited to remaining Win32 spelling
        // differences.
        fn normalize(path: &Path) -> String {
            let mut normalized = path.to_string_lossy().replace('/', "\\");
            if let Some(stripped) = normalized.strip_prefix(r"\\?\") {
                normalized = stripped.to_owned();
            }
            normalized.to_ascii_lowercase()
        }
        return normalize(left) == normalize(right);
    }
}



#[inline]
fn ensure_peer_executable_matches_current_by_pid(peer_pid: u32, postfix: &str) -> ResultType<()> {
    let peer_exe = peer_exe_canonical_path_by_pid(peer_pid)?;
    let current_exe = current_exe_canonical_path()?;
    if executable_paths_match(&peer_exe, &current_exe) {
        return Ok(());
    }
    if windows_portable_service_ipc_allows_logon_helper_executable(&peer_exe, postfix) {
        return Ok(());
    }
    bail!(
        "Peer executable path mismatch on ipc channel '{}': peer_pid={}, peer_exe='{}', current_exe='{}'",
        postfix,
        peer_pid,
        peer_exe.display(),
        current_exe.display()
    );
}

#[inline]
pub(crate) fn ensure_peer_executable_matches_current_by_pid_opt(
    peer_pid: Option<u32>,
    postfix: &str,
) -> ResultType<()> {
    let peer_pid = peer_pid.ok_or_else(|| {
        anyhow::anyhow!("Failed to resolve peer pid on ipc channel '{}'", postfix)
    })?;
    ensure_peer_executable_matches_current_by_pid(peer_pid, postfix)
}


const UNAUTHORIZED_IPC_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);



#[inline]
pub(crate) fn log_rejected_windows_ipc_connection(
    postfix: &str,
    peer_pid: Option<u32>,
    peer_session_id: Option<u32>,
    expected_session_id: Option<u32>,
    peer_is_system: Option<bool>,
    peer_is_elevated: Option<bool>,
) {
    hbb_common::throttled_log!(
        UNAUTHORIZED_IPC_LOG_INTERVAL,
        warn,
        "Rejected unauthorized connection on ipc channel: postfix={}, peer_pid={:?}, peer_session_id={:?}, expected_session_id={:?}, peer_is_system={:?}, peer_is_elevated={:?}",
        postfix,
        peer_pid,
        peer_session_id,
        expected_session_id,
        peer_is_system,
        peer_is_elevated
    );
}



pub(crate) fn authorize_windows_main_ipc_connection(stream: &Connection, postfix: &str) -> bool {
    let (
        authorized,
        peer_pid,
        peer_session_id,
        server_session_id,
        peer_is_system,
        peer_is_elevated,
    ) = stream.server_authorization_status();
    if !authorized {
        log_rejected_windows_ipc_connection(
            postfix,
            peer_pid,
            peer_session_id,
            server_session_id,
            peer_is_system,
            peer_is_elevated,
        );
        return false;
    }
    if let Err(err) = ensure_peer_executable_matches_current_by_pid_opt(peer_pid, postfix) {
        log::warn!(
            "Rejected unauthorized connection on ipc channel due to executable mismatch: postfix={}, peer_pid={:?}, err={}",
            postfix,
            peer_pid,
            err
        );
        return false;
    }
    true
}

pub(crate) fn authorize_windows_portable_service_ipc_connection(
    stream: &Connection,
    postfix: &str,
) -> bool {
    // Portable service IPC policy:
    // - only SYSTEM peers are authorized by is_allowed_windows_portable_service_peer()
    // - expected_session_id is still collected for diagnostics and identity checks
    // - final privilege boundary is enforced by named-pipe ACL + one-time token handshake
    // - when peer identity is unavailable on some hosts, executable verification remains
    //   best-effort telemetry (not fail-closed) to avoid breaking valid SYSTEM bootstrap
    //   flows that cannot be fully introspected
    let expected_session_id = crate::platform::windows::get_current_process_session_id();
    let (authorized, peer_pid, peer_session_id, peer_is_system) =
        stream.portable_service_authorization_status_for_session(expected_session_id);
    if !authorized {
        // Session lookup may succeed while SYSTEM identity lookup fails, so only the
        // SYSTEM identity result determines whether peer identity is unavailable here.
        // Don't use `peer_pid.is_some() && peer_session_id.is_none() && peer_is_system.is_none();` here.
        let identity_unavailable = peer_pid.is_some() && peer_is_system.is_none();
        if identity_unavailable {
            // In portable-service startup, resolving SYSTEM peer identity may fail on some hosts.
            // `ProcessIdToSessionId` can still succeed while `OpenProcessToken(TOKEN_QUERY)` is
            // denied by the peer token DACL or missing privileges. Treat that partial identity
            // failure as unavailable and defer final authorization to pipe ACL + token handshake.
            if let Err(err) = ensure_peer_executable_matches_current_by_pid_opt(peer_pid, postfix) {
                log::warn!(
                    "Portable service ipc peer identity unavailable and executable verification failed; continue with ACL+token-gated flow: postfix={}, peer_pid={:?}, err={}",
                    postfix,
                    peer_pid,
                    err
                );
            } else {
                log::warn!(
                    "Portable service ipc peer identity unavailable; executable verification matched, continue with ACL+token-gated flow: postfix={}, peer_pid={:?}, expected_session_id={:?}",
                    postfix,
                    peer_pid,
                    expected_session_id
                );
            }
            return true;
        }
        log::warn!(
            "Rejected unauthorized connection on portable service ipc channel: postfix={}, peer_pid={:?}, peer_session_id={:?}, expected_session_id={:?}, peer_is_system={:?}",
            postfix,
            peer_pid,
            peer_session_id,
            expected_session_id,
            peer_is_system
        );
        return false;
    }
    true
}


impl ConnectionTmpl<parity_tokio_ipc::Connection> {
    fn peer_pid(&self) -> Option<u32> {
        let pipe_handle = self.inner.get_ref().as_raw_handle();
        if pipe_handle.is_null() {
            return None;
        }
        let mut pid = 0u32;
        let ok = unsafe { GetNamedPipeClientProcessId(HANDLE(pipe_handle), &mut pid as *mut u32) }
            .is_ok();
        if ok && pid != 0 {
            Some(pid)
        } else {
            None
        }
    }

    fn server_authorization_status(
        &self,
    ) -> (
        bool,
        Option<u32>,
        Option<u32>,
        Option<u32>,
        Option<bool>,
        Option<bool>,
    ) {
        let peer_pid = self.peer_pid();
        let server_session_id = crate::platform::windows::get_current_process_session_id();
        let peer_session_id =
            peer_pid.and_then(crate::platform::windows::get_session_id_of_process);
        let peer_is_system_result =
            peer_pid.map(crate::platform::windows::is_process_running_as_system);
        let peer_is_system = peer_is_system_result
            .as_ref()
            .and_then(|r| r.as_ref().ok().copied());
        let session_authorized = is_allowed_windows_session_scoped_peer(
            peer_is_system.unwrap_or(false),
            peer_session_id,
            server_session_id,
        );
        let peer_is_elevated_result = if session_authorized {
            None
        } else {
            peer_pid.map(|pid| crate::platform::windows::is_elevated(Some(pid)))
        };
        let peer_is_elevated = peer_is_elevated_result
            .as_ref()
            .and_then(|r| r.as_ref().ok().copied());
        if server_session_id.is_none()
            && !peer_is_system.unwrap_or(false)
            && !peer_is_elevated.unwrap_or(false)
        {
            // When the server session id cannot be determined, the session-id allow-path is
            // disabled and only privileged peers can be authorized.
            log::debug!(
                "IPC authorization: server session id unavailable; rejecting non-privileged peer, peer_pid={:?}, peer_session_id={:?}",
                peer_pid,
                peer_session_id
            );
        }
        // Main IPC trusts same-session peers, LocalSystem, and elevated administrators.
        // Service-scoped IPC channels keep their own stricter authorization paths.
        let authorized = session_authorized || peer_is_elevated.unwrap_or(false);
        if !authorized {
            if let (Some(pid), Some(Err(err))) = (peer_pid, peer_is_system_result.as_ref()) {
                log::debug!(
                    "Failed to determine whether peer process is SYSTEM, pid={}, err={}",
                    pid,
                    err
                );
            }
            if let (Some(pid), Some(Err(err))) = (peer_pid, peer_is_elevated_result.as_ref()) {
                log::debug!(
                    "Failed to determine whether peer process is elevated, pid={}, err={}",
                    pid,
                    err
                );
            }
        }
        (
            authorized,
            peer_pid,
            peer_session_id,
            server_session_id,
            peer_is_system,
            peer_is_elevated,
        )
    }

    pub(crate) fn service_authorization_status_for_session(
        &self,
        expected_active_session_id: Option<u32>,
    ) -> (bool, Option<u32>, Option<u32>, Option<bool>) {
        let peer_pid = self.peer_pid();
        let peer_session_id =
            peer_pid.and_then(crate::platform::windows::get_session_id_of_process);
        let peer_is_system_result =
            peer_pid.map(crate::platform::windows::is_process_running_as_system);
        let peer_is_system = peer_is_system_result
            .as_ref()
            .and_then(|r| r.as_ref().ok().copied());
        let authorized = is_allowed_windows_session_scoped_peer(
            peer_is_system.unwrap_or(false),
            peer_session_id,
            expected_active_session_id,
        );
        if !authorized {
            if let (Some(pid), Some(Err(err))) = (peer_pid, peer_is_system_result.as_ref()) {
                log::debug!(
                    "Failed to determine whether peer process is SYSTEM, pid={}, err={}",
                    pid,
                    err
                );
            }
        }
        (authorized, peer_pid, peer_session_id, peer_is_system)
    }

    pub(crate) fn portable_service_authorization_status_for_session(
        &self,
        expected_active_session_id: Option<u32>,
    ) -> (bool, Option<u32>, Option<u32>, Option<bool>) {
        // Portable-service policy:
        // only SYSTEM peers are allowed.
        let (_service_authorized, peer_pid, peer_session_id, peer_is_system) =
            self.service_authorization_status_for_session(expected_active_session_id);
        (
            is_allowed_windows_portable_service_peer(
                peer_is_system,
                peer_session_id,
                expected_active_session_id,
            ),
            peer_pid,
            peer_session_id,
            peer_is_system,
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_windows_server_peer_policy() {
        assert!(super::is_allowed_windows_session_scoped_peer(
            true, None, None
        ));
        assert!(super::is_allowed_windows_session_scoped_peer(
            false,
            Some(1),
            Some(1)
        ));
        assert!(!super::is_allowed_windows_session_scoped_peer(
            false,
            Some(1),
            Some(2)
        ));
        assert!(!super::is_allowed_windows_session_scoped_peer(
            false,
            None,
            Some(1)
        ));
    }

    #[test]
    fn test_windows_portable_service_peer_policy() {
        assert!(super::is_allowed_windows_portable_service_peer(
            Some(true),
            None,
            None
        ));
        assert!(!super::is_allowed_windows_portable_service_peer(
            Some(false),
            Some(1),
            Some(1)
        ));
        assert!(!super::is_allowed_windows_portable_service_peer(
            Some(false),
            Some(1),
            Some(2)
        ));
        assert!(!super::is_allowed_windows_portable_service_peer(
            None,
            Some(1),
            Some(1)
        ));
    }
}
