//! Plugin registration, host handshake, and revoke lifecycle.

use anvil_ipc::GatewayClient;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::host_session::{self, HostEvent, HostSession};

pub(crate) const DEBUG_PLUGIN_ID: &str = "debug";
pub(crate) const DEBUG_PERMISSIONS: &[&str] = &["sys.exec"];

pub struct PluginSession {
    plugin_id: String,
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
        Self::start_mode(gateway, workspace, plugin_id, permissions, false)
    }

    pub(crate) fn start_oom(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
    ) -> Result<Self, String> {
        Self::start_mode(gateway, workspace, plugin_id, permissions, true)
    }

    fn start_mode(
        gateway: &mut GatewayClient,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
        oom: bool,
    ) -> Result<Self, String> {
        let token = host_session::new_token();
        let hash = Sha256::digest(token.0);
        gateway
            .register_plugin(plugin_id, &hash.into(), permissions)
            .map_err(|error| format!("register plugin: {error}"))?;
        let host = if oom {
            HostSession::start_oom_with_token(workspace, token)
        } else {
            HostSession::start_with_token(workspace, token)
        };
        match host {
            Ok(host) => Ok(Self {
                plugin_id: plugin_id.to_owned(),
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

    pub(crate) fn stop(&mut self, gateway: &mut GatewayClient) {
        if self.stopped {
            return;
        }
        self.host.stop();
        let _ = gateway.revoke_plugin(&self.plugin_id);
        self.stopped = true;
    }
}

impl Drop for PluginSession {
    fn drop(&mut self) {
        self.host.stop();
    }
}
