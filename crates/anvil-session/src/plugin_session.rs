//! Plugin registration, host handshake, and revoke lifecycle.

use anvil_ipc::GatewayClient;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::host_session::{self, HostEvent, HostSession};

pub const DEBUG_PLUGIN_ID: &str = "debug";
pub const DEBUG_PERMISSIONS: &[&str] = &["sys.exec"];

pub struct PluginSession {
    host: HostSession,
    stopped: bool,
}

impl PluginSession {
    pub(crate) fn start(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
    ) -> Result<Self, String> {
        Self::start_mode(gateway, workspace, plugin_id, permissions, None, false)
    }

    pub(crate) fn start_oom(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
    ) -> Result<Self, String> {
        Self::start_mode(gateway, workspace, plugin_id, permissions, None, true)
    }

    pub(crate) fn start_plugin(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: &Path,
        permissions: &[String],
    ) -> Result<Self, String> {
        Self::start_mode(
            gateway,
            workspace,
            plugin_id,
            permissions,
            Some(plugin_root),
            false,
        )
    }

    fn start_mode(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
        plugin_root: Option<&Path>,
        oom: bool,
    ) -> Result<Self, String> {
        let token = host_session::new_token();
        let hash = Sha256::digest(token.0);
        gateway
            .register_plugin(plugin_id, &hash.into(), permissions)
            .map_err(|error| format!("register plugin: {error}"))?;
        let gateway_socket = gateway.socket_path().to_path_buf();
        let host = if oom {
            HostSession::start_oom_with_token(workspace, token, Some(&gateway_socket))
        } else if let Some(plugin_root) = plugin_root {
            HostSession::start_with_token_plugin(
                workspace,
                plugin_root,
                token,
                Some(&gateway_socket),
            )
        } else {
            HostSession::start_with_token(workspace, token, Some(&gateway_socket))
        };
        match host {
            Ok(host) => Ok(Self {
                host,
                stopped: false,
            }),
            Err(error) => {
                let _ = gateway.revoke_plugin(plugin_id);
                Err(error)
            }
        }
    }

    pub(crate) fn poll(&mut self) -> Vec<HostEvent> {
        self.host.poll()
    }

    pub(crate) fn exec(&self) -> bool {
        self.host.exec()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.host.has_pending()
    }

    pub(crate) fn drain(&mut self, timeout: std::time::Duration) {
        self.host.drain(timeout);
    }

    pub(crate) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.host.stop();
        self.stopped = true;
    }
}

impl Drop for PluginSession {
    fn drop(&mut self) {
        self.host.stop();
    }
}
