//! Typed, source-level module contract for trusted Anvil distributions.
//!
//! Modules are compiled in and registered explicitly. They are not runtime
//! plugins and there is no stable Rust ABI.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use anvil_session::command::{COMMANDS, CommandId, CommandSpec};

/// Version of the typed module contract.
pub const CONTRACT_VERSION: u32 = 1;

/// Inclusive range of module contract versions supported by a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractRange {
    pub min: u32,
    pub max: u32,
}

impl ContractRange {
    pub const fn new(min: u32, max: u32) -> Self {
        Self { min, max }
    }

    pub const fn current() -> Self {
        Self::new(CONTRACT_VERSION, CONTRACT_VERSION)
    }

    pub const fn supports(self, version: u32) -> bool {
        self.min <= version && version <= self.max
    }
}

/// Stable module identity in `<vendor>.<name>` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleId(&'static str);

impl ModuleId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    pub fn is_valid(self) -> bool {
        let Some((vendor, name)) = self.0.split_once('.') else {
            return false;
        };
        !vendor.is_empty()
            && !name.is_empty()
            && !name.contains('.')
            && vendor.bytes().chain(name.bytes()).all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
    }
}

/// Declarative metadata supplied by a module.
#[derive(Clone, Copy, Debug)]
pub struct ModuleMetadata {
    pub id: ModuleId,
    pub name: &'static str,
    pub contract: ContractRange,
    pub commands: &'static [CommandSpec],
}

/// One module compiled into a distribution.
pub trait Module {
    fn metadata(&self) -> ModuleMetadata;

    /// Register services or extension points that are not declared by metadata.
    ///
    /// Metadata commands are registered by [`ModuleRegistry::register`] before
    /// this hook runs. Returning an error rolls back the module and every
    /// command it registered.
    fn register(&self, _registry: &mut ModuleRegistry) -> Result<(), ModuleError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleError {
    InvalidId(ModuleId),
    ContractMismatch {
        module: ModuleId,
        required: ContractRange,
    },
    DuplicateModule(ModuleId),
    InvalidCommand {
        module: ModuleId,
        command: CommandId,
        reason: &'static str,
    },
    DuplicateCommand(CommandId),
    KernelCommand(CommandId),
    Registration {
        module: ModuleId,
        message: String,
    },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(module) => write!(formatter, "invalid module id: {}", module.0),
            Self::ContractMismatch { module, required } => write!(
                formatter,
                "module {} requires contract {}..={} but this binary provides {}",
                module.0, required.min, required.max, CONTRACT_VERSION
            ),
            Self::DuplicateModule(module) => write!(formatter, "duplicate module id: {}", module.0),
            Self::InvalidCommand {
                module,
                command,
                reason,
            } => write!(
                formatter,
                "module {} command {}: {}",
                module.0,
                command.as_str(),
                reason
            ),
            Self::DuplicateCommand(command) => {
                write!(formatter, "duplicate command id: {}", command.as_str())
            }
            Self::KernelCommand(command) => {
                write!(
                    formatter,
                    "module command shadows kernel command: {}",
                    command.as_str()
                )
            }
            Self::Registration { module, message } => {
                write!(
                    formatter,
                    "module {} registration failed: {}",
                    module.0, message
                )
            }
        }
    }
}

impl std::error::Error for ModuleError {}

/// Registry owned by a composition crate.
#[derive(Default)]
pub struct ModuleRegistry {
    modules: BTreeMap<ModuleId, ModuleMetadata>,
    commands: HashMap<CommandId, (&'static CommandSpec, ModuleId)>,
    registering: Option<ModuleId>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<M: Module>(&mut self, module: &M) -> Result<(), ModuleError> {
        let metadata = module.metadata();
        let id = metadata.id;
        if !id.is_valid() {
            return Err(ModuleError::InvalidId(id));
        }
        if !metadata.contract.supports(CONTRACT_VERSION) {
            return Err(ModuleError::ContractMismatch {
                module: id,
                required: metadata.contract,
            });
        }
        if self.modules.contains_key(&id) {
            return Err(ModuleError::DuplicateModule(id));
        }
        let mut pending = BTreeSet::new();
        for spec in metadata.commands {
            if !pending.insert(spec.id.as_str()) {
                return Err(ModuleError::DuplicateCommand(spec.id));
            }
            self.validate_command(id, spec)?;
        }
        self.modules.insert(id, metadata);
        for spec in metadata.commands {
            self.commands.insert(spec.id, (spec, id));
        }
        self.registering = Some(id);
        let registration = module.register(self);
        self.registering = None;
        if let Err(error) = registration {
            self.modules.remove(&id);
            self.commands.retain(|_, (_, owner)| *owner != id);
            return Err(ModuleError::Registration {
                module: id,
                message: error.to_string(),
            });
        }
        Ok(())
    }

    /// Register an additional command from inside [`Module::register`].
    ///
    /// The owning module is the module currently being registered. A module
    /// cannot add a command for another module's namespace.
    pub fn add_command(&mut self, spec: &'static CommandSpec) -> Result<(), ModuleError> {
        let Some(module) = self.registering else {
            return Err(ModuleError::Registration {
                module: ModuleId::new("unknown"),
                message: "add_command is only valid during module registration".to_owned(),
            });
        };
        self.validate_command(module, spec)?;
        self.commands.insert(spec.id, (spec, module));
        Ok(())
    }

    pub fn contains(&self, module: ModuleId) -> bool {
        self.modules.contains_key(&module)
    }

    pub fn command_owner(&self, command: &str) -> Option<ModuleId> {
        self.commands
            .iter()
            .find_map(|(id, (_, owner))| (id.as_str() == command).then_some(*owner))
    }

    pub fn command(&self, command: &str) -> Option<&'static CommandSpec> {
        self.commands
            .iter()
            .find_map(|(id, (spec, _))| (id.as_str() == command).then_some(*spec))
    }

    pub fn modules(&self) -> impl Iterator<Item = ModuleId> + '_ {
        self.modules.keys().copied()
    }

    fn validate_command(
        &self,
        module: ModuleId,
        spec: &'static CommandSpec,
    ) -> Result<(), ModuleError> {
        let command = spec.id;
        let prefix = format!("{}.", module.0);
        if !command.as_str().starts_with(&prefix) {
            return Err(ModuleError::InvalidCommand {
                module,
                command,
                reason: "command id must start with the module id",
            });
        }
        if spec.contexts.is_empty() {
            return Err(ModuleError::InvalidCommand {
                module,
                command,
                reason: "command must declare at least one context",
            });
        }
        if COMMANDS.iter().any(|kernel| kernel.id == command) {
            return Err(ModuleError::KernelCommand(command));
        }
        if self.commands.contains_key(&command) {
            return Err(ModuleError::DuplicateCommand(command));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_session::command::{CommandContext, CommandId};

    const COMMANDS: &[CommandSpec] = &[CommandSpec {
        id: CommandId::new("official.demo.run"),
        contexts: &[CommandContext::Normal],
    }];

    struct Demo;
    impl Module for Demo {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: ModuleId::new("official.demo"),
                name: "Demo",
                contract: ContractRange::current(),
                commands: COMMANDS,
            }
        }
    }

    const BROKEN_COMMANDS: &[CommandSpec] = &[CommandSpec {
        id: CommandId::new("official.broken.run"),
        contexts: &[CommandContext::Normal],
    }];

    struct Broken;
    impl Module for Broken {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: ModuleId::new("official.broken"),
                name: "Broken",
                contract: ContractRange::current(),
                commands: BROKEN_COMMANDS,
            }
        }

        fn register(&self, _registry: &mut ModuleRegistry) -> Result<(), ModuleError> {
            Err(ModuleError::Registration {
                module: ModuleId::new("official.broken"),
                message: "no".to_owned(),
            })
        }
    }

    #[test]
    fn registers_a_valid_module_and_command() {
        let mut registry = ModuleRegistry::new();
        registry.register(&Demo).expect("register");
        assert!(registry.contains(ModuleId::new("official.demo")));
        assert_eq!(
            registry.command_owner("official.demo.run"),
            Some(ModuleId::new("official.demo"))
        );
    }

    struct Future;
    impl Module for Future {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: ModuleId::new("official.future"),
                name: "Future",
                contract: ContractRange::new(2, 3),
                commands: &[],
            }
        }
    }

    const EXTRA_COMMANDS: &[CommandSpec] = &[CommandSpec {
        id: CommandId::new("official.extra.run"),
        contexts: &[CommandContext::Normal, CommandContext::Pending],
    }];

    struct Extra;
    impl Module for Extra {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: ModuleId::new("official.extra"),
                name: "Extra",
                contract: ContractRange::current(),
                commands: &[],
            }
        }

        fn register(&self, registry: &mut ModuleRegistry) -> Result<(), ModuleError> {
            registry.add_command(&EXTRA_COMMANDS[0])
        }
    }

    struct Impostor;
    impl Module for Impostor {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                id: ModuleId::new("official.impostor"),
                name: "Impostor",
                contract: ContractRange::current(),
                commands: &[],
            }
        }

        fn register(&self, registry: &mut ModuleRegistry) -> Result<(), ModuleError> {
            registry.add_command(&EXTRA_COMMANDS[0])
        }
    }

    #[test]
    fn add_command_keeps_the_spec_and_current_module_owner() {
        let mut registry = ModuleRegistry::new();
        registry.register(&Extra).expect("register");
        let spec = registry.command("official.extra.run").expect("command");
        assert_eq!(spec.contexts.len(), 2);
        assert_eq!(
            registry.command_owner("official.extra.run"),
            Some(ModuleId::new("official.extra"))
        );

        let standalone = ModuleRegistry::new();
        let mut standalone = standalone;
        assert!(standalone.add_command(&EXTRA_COMMANDS[0]).is_err());
    }

    #[test]
    fn add_command_rejects_another_module_namespace_and_rolls_back() {
        let mut registry = ModuleRegistry::new();
        assert!(registry.register(&Impostor).is_err());
        assert!(!registry.contains(ModuleId::new("official.impostor")));
        assert_eq!(registry.command_owner("official.extra.run"), None);
    }

    #[test]
    fn rejects_invalid_ids_and_contract_versions() {
        assert!(!ModuleId::new("invalid").is_valid());
        assert!(!ModuleId::new("Vendor.Name").is_valid());
        assert!(!ContractRange::new(2, 3).supports(1));
        assert!(ContractRange::new(2, 3).supports(2));

        let mut registry = ModuleRegistry::new();
        assert!(matches!(
            registry.register(&Future),
            Err(ModuleError::ContractMismatch { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_module_and_command_ids() {
        let mut registry = ModuleRegistry::new();
        registry.register(&Demo).unwrap();
        assert!(matches!(
            registry.register(&Demo),
            Err(ModuleError::DuplicateModule(_))
        ));

        const COLLISION_COMMANDS: &[CommandSpec] = &[
            CommandSpec {
                id: CommandId::new("official.collision.run"),
                contexts: &[CommandContext::Normal],
            },
            CommandSpec {
                id: CommandId::new("official.collision.run"),
                contexts: &[CommandContext::Normal],
            },
        ];

        struct Collision;
        impl Module for Collision {
            fn metadata(&self) -> ModuleMetadata {
                ModuleMetadata {
                    id: ModuleId::new("official.collision"),
                    name: "Collision",
                    contract: ContractRange::current(),
                    commands: COLLISION_COMMANDS,
                }
            }
        }
        assert!(matches!(
            registry.register(&Collision),
            Err(ModuleError::DuplicateCommand(_))
        ));
    }

    #[test]
    fn rejects_kernel_command_shadowing() {
        const SHADOW_COMMANDS: &[CommandSpec] = &[CommandSpec {
            id: CommandId::new("lsp.hover"),
            contexts: &[CommandContext::Normal],
        }];

        struct Shadow;
        impl Module for Shadow {
            fn metadata(&self) -> ModuleMetadata {
                ModuleMetadata {
                    id: ModuleId::new("official.shadow"),
                    name: "Shadow",
                    contract: ContractRange::current(),
                    commands: SHADOW_COMMANDS,
                }
            }
        }
        // A kernel id cannot satisfy the module prefix, so it fails instead of
        // becoming a silent shadow.
        let mut registry = ModuleRegistry::new();
        assert!(matches!(
            registry.register(&Shadow),
            Err(ModuleError::InvalidCommand { .. })
        ));
    }

    #[test]
    fn failed_registration_rolls_back_commands() {
        let mut registry = ModuleRegistry::new();
        assert!(registry.register(&Broken).is_err());
        assert!(!registry.contains(ModuleId::new("official.broken")));
        assert_eq!(registry.command_owner("official.broken.run"), None);
    }
}
