//! Plugin registration, host handshake, and revoke lifecycle.

use anvil_ipc::PluginGateway;

/// Session-facing gateway port; the concrete client is injected by the frontend.
pub(crate) type GatewayPort = dyn PluginGateway;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::host_session::{HostEvent, HostSession, SendOutcome};
use crate::plugin_runtime::{HostSpawner, HostStart, HostToken};

pub const DEBUG_PLUGIN_ID: &str = "debug";
pub const DEBUG_PERMISSIONS: &[&str] = &["sys.exec"];

pub struct PluginSession {
    host: HostSession,
    stopped: bool,
    token_hash: [u8; 32],
    identities: Arc<Mutex<HashMap<[u8; 32], String>>>,
}

impl PluginSession {
    pub(crate) fn start(
        gateway: &mut GatewayPort,
        spawner: &dyn HostSpawner,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
        editor_socket: Option<&Path>,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Self, String> {
        Self::start_mode(
            gateway,
            spawner,
            workspace,
            plugin_id,
            permissions,
            editor_socket,
            None,
            None,
            false,
            identities,
        )
    }

    pub(crate) fn start_oom(
        gateway: &mut GatewayPort,
        spawner: &dyn HostSpawner,
        workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
        editor_socket: Option<&Path>,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Self, String> {
        Self::start_mode(
            gateway,
            spawner,
            workspace,
            plugin_id,
            permissions,
            editor_socket,
            None,
            None,
            true,
            identities,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_plugin(
        gateway: &mut GatewayPort,
        spawner: &dyn HostSpawner,
        workspace: &Path,
        plugin_id: &str,
        plugin_root: &Path,
        permissions: &[String],
        editor_socket: Option<&Path>,
        command: Option<&str>,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Self, String> {
        Self::start_mode(
            gateway,
            spawner,
            workspace,
            plugin_id,
            permissions,
            editor_socket,
            Some(plugin_root),
            command,
            false,
            identities,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_mode(
        gateway: &mut GatewayPort,
        spawner: &dyn HostSpawner,
        _workspace: &Path,
        plugin_id: &str,
        permissions: &[String],
        editor_socket: Option<&Path>,
        plugin_root: Option<&Path>,
        command: Option<&str>,
        oom: bool,
        identities: &Arc<Mutex<HashMap<[u8; 32], String>>>,
    ) -> Result<Self, String> {
        let token = HostToken::new();
        let hash = Sha256::digest(token.0);
        let token_hash: [u8; 32] = hash.into();
        gateway
            .register_plugin(plugin_id, &hash.into(), permissions)
            .map_err(|error| format!("register plugin: {error}"))?;
        match identities.lock() {
            Ok(mut identities) => {
                identities.insert(token_hash, plugin_id.to_owned());
            }
            Err(_) => {
                let _ = gateway.revoke_plugin(plugin_id);
                return Err("editor identity lock poisoned".to_owned());
            }
        }
        let gateway_socket = gateway.socket_path().to_path_buf();
        let host = HostSession::start(
            spawner,
            HostStart {
                plugin_id,
                plugin_root,
                token,
                gateway_socket: Some(&gateway_socket),
                editor_socket,
                command,
                allocate_mb: oom.then_some(300),
            },
        );
        match host {
            Ok(host) => Ok(Self {
                host,
                stopped: false,
                token_hash,
                identities: Arc::clone(identities),
            }),
            Err(error) => {
                let _ = gateway.revoke_plugin(plugin_id);
                if let Ok(mut identities) = identities.lock() {
                    identities.remove(&token_hash);
                }
                Err(error)
            }
        }
    }

    pub(crate) fn poll(&mut self) -> Vec<HostEvent> {
        self.host.poll()
    }

    pub(crate) fn exec(&self) -> SendOutcome {
        self.host.exec()
    }

    pub(crate) fn event(
        &self,
        subscription_id: u64,
        event: &str,
        sequence: u64,
        payload: serde_json::Value,
    ) -> bool {
        self.host.event(subscription_id, event, sequence, payload)
    }

    pub(crate) fn event_stats(&self) -> bool {
        self.host.event_stats()
    }

    pub(crate) fn provider(&self, id: u64, handler: &str, query: &str, limit: u32) -> SendOutcome {
        self.host.provider(id, handler, query, limit)
    }

    pub(crate) fn provider_action(&self, id: u64, handler: &str, value: &str) -> SendOutcome {
        self.host.provider_action(id, handler, value)
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
        self.revoke_identity();
        self.stopped = true;
    }

    fn revoke_identity(&self) {
        if let Ok(mut identities) = self.identities.lock() {
            identities.remove(&self.token_hash);
        }
    }
}

impl Drop for PluginSession {
    fn drop(&mut self) {
        self.host.stop();
        self.revoke_identity();
    }
}
