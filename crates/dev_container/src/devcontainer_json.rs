use std::{collections::HashMap, fmt::Display, path::Path, sync::Arc};

use crate::{command_json::CommandRunner, devcontainer_api::DevContainerError};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json_lenient::Value;
use util::command::Command;
use util::redact::is_valid_environment_name;

#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Clone)]
#[serde(untagged)]
pub(crate) enum ForwardPort {
    Number(u16),
    String(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PortAttributeProtocol {
    Https,
    Http,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OnAutoForward {
    #[default]
    Notify,
    OpenBrowser,
    OpenBrowserOnce,
    OpenPreview,
    Silent,
    Ignore,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PortAttributes {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    on_auto_forward: OnAutoForward,
    #[serde(default)]
    elevate_if_needed: bool,
    #[serde(default)]
    require_local_port: bool,
    #[serde(default)]
    protocol: Option<PortAttributeProtocol>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UserEnvProbe {
    None,
    InteractiveShell,
    LoginShell,
    LoginInteractiveShell,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShutdownAction {
    None,
    StopContainer,
    StopCompose,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MountDefinition {
    #[serde(default)]
    pub(crate) source: Option<String>,
    pub(crate) target: String,
    #[serde(rename = "type")]
    pub(crate) mount_type: Option<String>,
}

impl Display for MountDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mount_type = self.mount_type.clone().unwrap_or_else(|| {
            if let Some(source) = &self.source {
                if source.starts_with('/')
                    || source.starts_with("\\\\")
                    || source.get(1..3) == Some(":\\")
                    || source.get(1..3) == Some(":/")
                {
                    return "bind".to_string();
                }
            }
            "volume".to_string()
        });
        write!(f, "type={}", mount_type)?;
        if let Some(source) = &self.source {
            write!(f, ",source={}", source)?;
        }
        write!(f, ",target={},consistency=cached", self.target)
    }
}

/// Represents the value associated with a feature ID in the `features` map of devcontainer.json.
///
/// Per the spec, the value can be:
/// - A boolean (`true` to enable with defaults)
/// - A string (shorthand for `{"version": "<value>"}`)
/// - An object mapping option names to string or boolean values
///
/// See: https://containers.dev/implementors/features/#devcontainerjson-properties
#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Clone)]
#[serde(untagged)]
pub(crate) enum FeatureOptions {
    Bool(bool),
    String(String),
    Options(HashMap<String, FeatureOptionValue>),
}

#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Clone)]
#[serde(untagged)]
pub(crate) enum FeatureOptionValue {
    Bool(bool),
    String(String),
}
impl std::fmt::Display for FeatureOptionValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureOptionValue::Bool(b) => write!(f, "{}", b),
            FeatureOptionValue::String(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq, Default)]
pub(crate) struct ZedCustomizationsWrapper {
    pub(crate) zed: ZedCustomization,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Default)]
pub(crate) struct ZedCustomization {
    #[serde(default)]
    pub(crate) extensions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContainerBuild {
    pub(crate) dockerfile: String,
    pub(crate) context: Option<String>,
    pub(crate) args: Option<HashMap<String, String>>,
    pub(crate) options: Option<Vec<String>>,
    pub(crate) target: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub(crate) cache_from: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
struct LifecycleScriptInternal {
    command: Option<String>,
    args: Vec<String>,
}

impl LifecycleScriptInternal {
    fn from_args(args: Vec<String>) -> Self {
        let command = args.get(0).map(|a| a.to_string());
        let remaining = args.iter().skip(1).map(|a| a.to_string()).collect();
        Self {
            command,
            args: remaining,
        }
    }
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct LifecycleScript {
    scripts: HashMap<String, LifecycleScriptInternal>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostRequirements {
    cpus: Option<u16>,
    memory: Option<String>,
    storage: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LifecycleCommand {
    InitializeCommand,
    OnCreateCommand,
    UpdateContentCommand,
    PostCreateCommand,
    PostStartCommand,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DevContainerBuildType {
    Image(String),
    Dockerfile(ContainerBuild),
    DockerCompose,
    None,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevContainer {
    pub(crate) image: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) remote_user: Option<String>,
    pub(crate) forward_ports: Option<Vec<ForwardPort>>,
    pub(crate) ports_attributes: Option<HashMap<String, PortAttributes>>,
    pub(crate) other_ports_attributes: Option<PortAttributes>,
    pub(crate) container_env: Option<HashMap<String, String>>,
    pub(crate) remote_env: Option<HashMap<String, String>>,
    pub(crate) container_user: Option<String>,
    #[serde(rename = "updateRemoteUserUID")]
    pub(crate) update_remote_user_uid: Option<bool>,
    user_env_probe: Option<UserEnvProbe>,
    pub(crate) override_command: Option<bool>,
    shutdown_action: Option<ShutdownAction>,
    pub(crate) init: Option<bool>,
    pub(crate) privileged: Option<bool>,
    pub(crate) cap_add: Option<Vec<String>>,
    pub(crate) security_opt: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_mount_definitions")]
    pub(crate) mounts: Option<Vec<MountDefinition>>,
    pub(crate) features: Option<HashMap<String, FeatureOptions>>,
    pub(crate) override_feature_install_order: Option<Vec<String>>,
    pub(crate) customizations: Option<ZedCustomizationsWrapper>,
    pub(crate) build: Option<ContainerBuild>,
    #[serde(default, deserialize_with = "deserialize_app_port")]
    pub(crate) app_port: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_mount_definition")]
    pub(crate) workspace_mount: Option<MountDefinition>,
    pub(crate) workspace_folder: Option<String>,
    pub(crate) run_args: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub(crate) docker_compose_file: Option<Vec<String>>,
    pub(crate) service: Option<String>,
    pub(crate) run_services: Option<Vec<String>>,
    pub(crate) initialize_command: Option<LifecycleScript>,
    pub(crate) on_create_command: Option<LifecycleScript>,
    pub(crate) update_content_command: Option<LifecycleScript>,
    pub(crate) post_create_command: Option<LifecycleScript>,
    pub(crate) post_start_command: Option<LifecycleScript>,
    pub(crate) post_attach_command: Option<LifecycleScript>,
    wait_for: Option<LifecycleCommand>,
    host_requirements: Option<HostRequirements>,
}

pub(crate) fn deserialize_devcontainer_json_to_value(
    json: &str,
) -> Result<serde_json_lenient::Value, DevContainerError> {
    serde_json_lenient::from_str(json).map_err(|e| {
        log::error!("Unable to deserialize json values: {e}");
        DevContainerError::DevContainerParseFailed
    })
}

pub(crate) fn deserialize_devcontainer_json_from_value(
    json: serde_json_lenient::Value,
) -> Result<DevContainer, DevContainerError> {
    serde_json_lenient::from_value(json).map_err(|e| {
        log::error!("Unable to deserialize devcontainer from json values: {e}");
        DevContainerError::DevContainerParseFailed
    })
}

pub(crate) fn deserialize_devcontainer_json(json: &str) -> Result<DevContainer, DevContainerError> {
    deserialize_devcontainer_json_to_value(json).and_then(deserialize_devcontainer_json_from_value)
}

impl DevContainer {
    pub(crate) fn build_type(&self) -> DevContainerBuildType {
        if let Some(image) = &self.image {
            DevContainerBuildType::Image(image.clone())
        } else if self.docker_compose_file.is_some() {
            DevContainerBuildType::DockerCompose
        } else if let Some(build) = &self.build {
            DevContainerBuildType::Dockerfile(build.clone())
        } else {
            DevContainerBuildType::None
        }
    }

    pub(crate) fn override_command(&self) -> bool {
        self.override_command.unwrap_or(!matches!(
            self.build_type(),
            DevContainerBuildType::DockerCompose
        ))
    }

    pub(crate) fn validate_environment_names(&self) -> Result<(), DevContainerError> {
        for (property, environment) in [
            ("containerEnv", self.container_env.as_ref()),
            ("remoteEnv", self.remote_env.as_ref()),
        ] {
            if environment.is_some_and(|environment| {
                environment
                    .keys()
                    .any(|name| !is_valid_environment_name(name))
            }) {
                return Err(DevContainerError::DevContainerValidationFailed(format!(
                    "{property} contains an invalid environment variable name"
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn validate_devcontainer_contents(&self) -> Result<(), DevContainerError> {
        self.validate_environment_names()?;

        match self.build_type() {
            DevContainerBuildType::Image(_) => Ok(()),
            DevContainerBuildType::Dockerfile(_) => {
                if (self.workspace_folder.is_some() && self.workspace_mount.is_none())
                    || (self.workspace_folder.is_none() && self.workspace_mount.is_some())
                {
                    return Err(DevContainerError::DevContainerValidationFailed(
                        "workspaceMount and workspaceFolder must both be defined, or neither defined"
                            .to_string(),
                    ));
                }
                Ok(())
            }
            DevContainerBuildType::DockerCompose => {
                if self.service.is_none() {
                    return Err(DevContainerError::DevContainerValidationFailed(
                        "must specify a connecting service for docker-compose".to_string(),
                    ));
                }
                Ok(())
            }
            DevContainerBuildType::None => Ok(()),
        }
    }
}

// Custom deserializer that parses the entire customizations object as a
// serde_json_lenient::Value first, then extracts the "zed" portion.
// This avoids a bug in serde_json_lenient's `ignore_value` codepath which
// does not handle trailing commas in skipped values.
impl<'de> Deserialize<'de> for ZedCustomizationsWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let zed = value
            .get("zed")
            .map(|zed_value| serde_json_lenient::from_value::<ZedCustomization>(zed_value.clone()))
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        Ok(ZedCustomizationsWrapper { zed })
    }
}

impl LifecycleScript {
    fn from_map(args: HashMap<String, Vec<String>>) -> Self {
        Self {
            scripts: args
                .into_iter()
                .map(|(k, v)| (k, LifecycleScriptInternal::from_args(v)))
                .collect(),
        }
    }
    fn shell_command(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
    }
    fn from_str(args: &str) -> Self {
        Self::from_args(Self::shell_command(args))
    }
    fn from_args(args: Vec<String>) -> Self {
        Self::from_map(HashMap::from([("default".to_string(), args)]))
    }
    pub fn script_commands(&self) -> HashMap<String, Command> {
        self.scripts
            .iter()
            .filter_map(|(k, v)| {
                if let Some(inner_command) = &v.command {
                    let mut command = Command::new(inner_command);
                    command.args(&v.args);
                    Some((k.clone(), command))
                } else {
                    log::warn!(
                        "Lifecycle script command {k}, value {:?} has no program to run. Skipping",
                        v
                    );
                    None
                }
            })
            .collect()
    }

    pub async fn run(
        &self,
        command_runnder: &Arc<dyn CommandRunner>,
        working_directory: &Path,
    ) -> Result<(), DevContainerError> {
        for (command_name, mut command) in self.script_commands() {
            log::debug!("Running script {command_name}");

            command.current_dir(working_directory);

            let output = command_runnder
                .run_command(&mut command)
                .await
                .map_err(|e| {
                    log::error!("Error running command {command_name}: {e}");
                    DevContainerError::CommandFailed(command_name.clone())
                })?;
            if !output.status.success() {
                let std_err = String::from_utf8_lossy(&output.stderr);
                log::error!(
                    "Command {command_name} produced a non-successful output. StdErr: {std_err}"
                );
            }
            let std_out = String::from_utf8_lossy(&output.stdout);
            log::debug!("Command {command_name} output:\n {std_out}");
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for LifecycleScript {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct LifecycleScriptVisitor;

        impl<'de> Visitor<'de> for LifecycleScriptVisitor {
            type Value = LifecycleScript;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string, an array of strings, or a map of arrays")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(LifecycleScript::from_str(value))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut array = Vec::new();
                while let Some(elem) = seq.next_element()? {
                    array.push(elem);
                }
                Ok(LifecycleScript::from_args(array))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut result = HashMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value: Value = map.next_value()?;
                    let script_args = match value {
                        Value::String(s) => LifecycleScript::shell_command(&s),
                        Value::Array(arr) => {
                            let strings: Vec<String> = arr
                                .into_iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect();
                            strings
                        }
                        _ => continue,
                    };
                    result.insert(key, script_args);
                }
                Ok(LifecycleScript::from_map(result))
            }
        }

        deserializer.deserialize_any(LifecycleScriptVisitor)
    }
}

fn deserialize_mount_definition<'de, D>(
    deserializer: D,
) -> Result<Option<MountDefinition>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MountItem {
        Object(MountDefinition),
        String(String),
    }

    let item = MountItem::deserialize(deserializer)?;

    let mount = match item {
        MountItem::Object(mount) => mount,
        MountItem::String(s) => {
            let mut source = None;
            let mut target = None;
            let mut mount_type = None;

            for part in s.split(',') {
                let part = part.trim();
                if let Some((key, value)) = part.split_once('=') {
                    match key.trim() {
                        "source" => source = Some(value.trim().to_string()),
                        "target" => target = Some(value.trim().to_string()),
                        "type" => mount_type = Some(value.trim().to_string()),
                        _ => {} // Ignore unknown keys
                    }
                }
            }

            let target = target
                .ok_or_else(|| D::Error::custom(format!("mount string missing 'target': {}", s)))?;

            MountDefinition {
                source,
                target,
                mount_type,
            }
        }
    };

    Ok(Some(mount))
}

fn deserialize_mount_definitions<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<MountDefinition>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MountItem {
        Object(MountDefinition),
        String(String),
    }

    let items = Vec::<MountItem>::deserialize(deserializer)?;
    let mut mounts = Vec::new();

    for item in items {
        match item {
            MountItem::Object(mount) => mounts.push(mount),
            MountItem::String(s) => {
                let mut source = None;
                let mut target = None;
                let mut mount_type = None;

                for part in s.split(',') {
                    let part = part.trim();
                    if let Some((key, value)) = part.split_once('=') {
                        match key.trim() {
                            "source" => source = Some(value.trim().to_string()),
                            "target" => target = Some(value.trim().to_string()),
                            "type" => mount_type = Some(value.trim().to_string()),
                            _ => {} // Ignore unknown keys
                        }
                    }
                }

                let target = target.ok_or_else(|| {
                    D::Error::custom(format!("mount string missing 'target': {}", s))
                })?;

                mounts.push(MountDefinition {
                    source,
                    target,
                    mount_type,
                });
            }
        }
    }

    Ok(Some(mounts))
}

fn deserialize_app_port<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        String(String),
        Int(u32),
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AppPort {
        Array(Vec<StringOrInt>),
        Single(StringOrInt),
    }

    fn normalize_port(value: StringOrInt) -> String {
        match value {
            StringOrInt::String(s) => {
                if s.contains(':') {
                    s
                } else {
                    format!("{s}:{s}")
                }
            }
            StringOrInt::Int(n) => format!("{n}:{n}"),
        }
    }

    match AppPort::deserialize(deserializer)? {
        AppPort::Single(value) => Ok(vec![normalize_port(value)]),
        AppPort::Array(values) => Ok(values.into_iter().map(normalize_port).collect()),
    }
}

fn deserialize_string_or_array<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrArray {
        String(String),
        Array(Vec<String>),
    }

    match StringOrArray::deserialize(deserializer)? {
        StringOrArray::String(s) => Ok(Some(vec![s])),
        StringOrArray::Array(b) => Ok(Some(b)),
    }
}
