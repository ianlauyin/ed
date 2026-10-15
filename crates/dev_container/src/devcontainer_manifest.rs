use std::{
    collections::HashMap,
    fmt::Debug,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use regex::Regex;

use fs::Fs;
use http_client::HttpClient;
use util::{ResultExt, command::Command, normalize_path};

use crate::{
    DevContainerConfig, DevContainerContext,
    command_json::{CommandRunner, DefaultCommandRunner},
    devcontainer_api::{DevContainerError, DevContainerUp},
    devcontainer_json::{
        ContainerBuild, DevContainer, DevContainerBuildType, FeatureOptions, ForwardPort,
        MountDefinition, deserialize_devcontainer_json, deserialize_devcontainer_json_from_value,
        deserialize_devcontainer_json_to_value,
    },
    docker::{
        Docker, DockerClient, DockerComposeConfig, DockerComposeService, DockerComposeServiceBuild,
        DockerComposeServicePort, DockerComposeVolume, DockerInspect, DockerPs,
    },
    features::{DevContainerFeatureJson, FeatureManifest, parse_oci_feature_ref},
    get_oci_token,
    oci::{TokenResponse, download_oci_tarball, get_oci_manifest},
    safe_id_lower,
};

enum ConfigStatus {
    Deserialized(DevContainer),
    VariableParsed(DevContainer),
}

enum ComposeUpBehavior {
    Resume,
    Create,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct DockerComposeResources {
    files: Vec<PathBuf>,
    config: DockerComposeConfig,
}

struct DevContainerManifest {
    http_client: Arc<dyn HttpClient>,
    fs: Arc<dyn Fs>,
    docker_client: Arc<dyn DockerClient>,
    command_runner: Arc<dyn CommandRunner>,
    raw_config: String,
    config: ConfigStatus,
    local_environment: HashMap<String, String>,
    local_project_directory: PathBuf,
    config_directory: PathBuf,
    file_name: String,
    root_image: Option<DockerInspect>,
    features_build_info: Option<FeaturesBuildInfo>,
    features: Vec<FeatureManifest>,
}
const DEFAULT_REMOTE_PROJECT_DIR: &str = "/workspaces";
impl DevContainerManifest {
    async fn new(
        context: &DevContainerContext,
        environment: HashMap<String, String>,
        docker_client: Arc<dyn DockerClient>,
        command_runner: Arc<dyn CommandRunner>,
        local_config: DevContainerConfig,
        local_project_path: &Path,
    ) -> Result<Self, DevContainerError> {
        let config_path = local_project_path.join(local_config.config_path.clone());
        log::debug!("parsing devcontainer json found in {config_path:?}");
        let devcontainer_contents = context.fs.load(&config_path).await.map_err(|e| {
            log::error!("Unable to read devcontainer contents: {e}");
            DevContainerError::DevContainerParseFailed
        })?;

        let devcontainer = deserialize_devcontainer_json(&devcontainer_contents)?;

        let devcontainer_directory = config_path.parent().ok_or_else(|| {
            log::error!("Dev container file should be in a directory");
            DevContainerError::NotInValidProject
        })?;
        let file_name = config_path
            .file_name()
            .and_then(|f| f.to_str())
            .ok_or_else(|| {
                log::error!("Dev container file has no file name, or is invalid unicode");
                DevContainerError::DevContainerParseFailed
            })?;

        Ok(Self {
            fs: context.fs.clone(),
            http_client: context.http_client.clone(),
            docker_client,
            command_runner,
            raw_config: devcontainer_contents,
            config: ConfigStatus::Deserialized(devcontainer),
            local_project_directory: local_project_path.to_path_buf(),
            local_environment: environment,
            config_directory: devcontainer_directory.to_path_buf(),
            file_name: file_name.to_string(),
            root_image: None,
            features_build_info: None,
            features: Vec::new(),
        })
    }

    fn devcontainer_id(&self) -> String {
        let mut labels = self.identifying_labels();
        labels.sort_by_key(|(key, _)| *key);

        let mut hasher = DefaultHasher::new();
        for (key, value) in &labels {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }

        format!("{:016x}", hasher.finish())
    }

    fn identifying_labels(&self) -> Vec<(&str, String)> {
        let labels = vec![
            (
                "devcontainer.local_folder",
                normalize_label_path(&self.local_project_directory.display().to_string()),
            ),
            (
                "devcontainer.config_file",
                normalize_label_path(&self.config_file().display().to_string()),
            ),
        ];
        labels
    }

    fn parse_nonremote_vars_for_content(
        &self,
        content: &str,
    ) -> Result<serde_json_lenient::Value, DevContainerError> {
        let mut value = deserialize_devcontainer_json_to_value(content)?;
        self.substitute_nonremote_vars_in_value(&mut value)?;
        Ok(value)
    }

    fn substitute_nonremote_vars_in_value(
        &self,
        value: &mut serde_json_lenient::Value,
    ) -> Result<(), DevContainerError> {
        let mut to_visit = vec![value];

        while let Some(value) = to_visit.pop() {
            use serde_json_lenient::Value;

            match value {
                Value::String(string) => {
                    *string = string
                        .replace("${devcontainerId}", &self.devcontainer_id())
                        .replace(
                            "${containerWorkspaceFolderBasename}",
                            &self.remote_workspace_base_name().unwrap_or_default(),
                        )
                        .replace(
                            "${localWorkspaceFolderBasename}",
                            &self.local_workspace_base_name()?,
                        )
                        .replace(
                            "${containerWorkspaceFolder}",
                            &self
                                .remote_workspace_folder()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default()
                                .replace('\\', "/"),
                        )
                        .replace(
                            "${localWorkspaceFolder}",
                            &self.local_workspace_folder().replace('\\', "/"),
                        );
                    *string = Self::replace_environment_variables(
                        string,
                        "localEnv",
                        &self.local_environment,
                    );
                }

                Value::Array(array) => to_visit.extend(array.iter_mut()),
                Value::Object(object) => to_visit.extend(object.values_mut()),

                Value::Null | Value::Bool(_) | Value::Number(_) => {}
            }
        }

        Ok(())
    }

    fn parse_nonremote_vars(&mut self) -> Result<(), DevContainerError> {
        let replaced_content = self.parse_nonremote_vars_for_content(&self.raw_config)?;
        let parsed_config = deserialize_devcontainer_json_from_value(replaced_content)?;

        self.config = ConfigStatus::VariableParsed(parsed_config);

        Ok(())
    }

    fn runtime_remote_env(
        &self,
        container_env: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>, DevContainerError> {
        let mut merged_remote_env = container_env.clone();
        // HOME is user-specific, and we will often not run as the image user
        merged_remote_env.remove("HOME");
        if let Some(mut remote_env) = self.dev_container().remote_env.clone() {
            remote_env.values_mut().for_each(|value| {
                *value = Self::replace_environment_variables(value, "containerEnv", &container_env)
            });
            for (k, v) in remote_env {
                merged_remote_env.insert(k, v);
            }
        }
        Ok(merged_remote_env)
    }

    fn replace_environment_variables(
        mut orig: &str,
        environment_source: &str,
        environment: &HashMap<String, String>,
    ) -> String {
        let mut replaced = String::with_capacity(orig.len());
        let prefix = format!("${{{environment_source}:");
        while let Some(start) = orig.find(&prefix) {
            let var_name_start = start + prefix.len();
            let Some(end) = orig[var_name_start..].find('}') else {
                // No closing `}` => malformed variable reference => paste as is.
                break;
            };
            let end = var_name_start + end;

            let (var_name_end, default_start) =
                if let Some(var_name_end) = orig[var_name_start..end].find(':') {
                    let var_name_end = var_name_start + var_name_end;
                    (var_name_end, var_name_end + 1)
                } else {
                    (end, end)
                };

            let var_name = &orig[var_name_start..var_name_end];
            if var_name.is_empty() {
                // Empty variable name => paste as is.
                replaced.push_str(&orig[..end + 1]);
                orig = &orig[end + 1..];
                continue;
            }
            let default = &orig[default_start..end];

            replaced.push_str(&orig[..start]);
            replaced.push_str(
                environment
                    .get(var_name)
                    .map(|value| value.as_str())
                    .unwrap_or(default),
            );
            orig = &orig[end + 1..];
        }
        replaced.push_str(orig);
        replaced
    }

    fn config_file(&self) -> PathBuf {
        self.config_directory.join(&self.file_name)
    }

    fn dev_container(&self) -> &DevContainer {
        match &self.config {
            ConfigStatus::Deserialized(dev_container) => dev_container,
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        }
    }

    async fn dockerfile_location(&self) -> Option<PathBuf> {
        let dev_container = self.dev_container();
        match dev_container.build_type() {
            DevContainerBuildType::Image(_) => None,
            DevContainerBuildType::Dockerfile(build) => {
                Some(self.config_directory.join(&build.dockerfile))
            }
            DevContainerBuildType::DockerCompose => {
                let Ok(docker_compose_manifest) = self.docker_compose_manifest().await else {
                    return None;
                };
                let Ok((_, main_service)) = find_primary_service(&docker_compose_manifest, self)
                else {
                    return None;
                };
                main_service.build.and_then(|b| {
                    let compose_file = docker_compose_manifest.files.first()?;
                    resolve_compose_dockerfile(
                        compose_file,
                        b.context.as_deref(),
                        b.dockerfile.as_deref()?,
                    )
                })
            }
            DevContainerBuildType::None => None,
        }
    }

    fn generate_features_image_tag(&self, dockerfile_build_path: String) -> String {
        let mut hasher = DefaultHasher::new();
        let prefix = match &self.dev_container().name {
            Some(name) => &safe_id_lower(name),
            None => "zed-dc",
        };
        let prefix = prefix.get(..6).unwrap_or(prefix);
        let prefix = prefix.trim_matches(|c: char| !c.is_alphanumeric());

        dockerfile_build_path.hash(&mut hasher);

        let hash = hasher.finish();
        format!("{}-{:x}-features", prefix, hash)
    }

    /// Gets the base image from the devcontainer with the following precedence:
    /// - The devcontainer image if an image is specified
    /// - The image sourced in the Dockerfile if a Dockerfile is specified
    /// - The image sourced in the docker-compose main service, if one is specified
    /// - The image sourced in the docker-compose main service dockerfile, if one is specified
    /// If no such image is available, return an error
    async fn get_base_image_from_config(&self) -> Result<String, DevContainerError> {
        match self.dev_container().build_type() {
            DevContainerBuildType::Image(image) => {
                return Ok(image);
            }
            DevContainerBuildType::Dockerfile(build) => {
                let dockerfile_contents = self.expanded_dockerfile_content().await?;
                return image_from_dockerfile(dockerfile_contents, &build.target).ok_or_else(
                    || {
                        log::error!("Unable to find base image in Dockerfile");
                        DevContainerError::DevContainerParseFailed
                    },
                );
            }
            DevContainerBuildType::DockerCompose => {
                let docker_compose_manifest = self.docker_compose_manifest().await?;
                let (_, main_service) = find_primary_service(&docker_compose_manifest, &self)?;

                if let Some(_) = main_service
                    .build
                    .as_ref()
                    .and_then(|b| b.dockerfile.as_ref())
                {
                    let dockerfile_contents = self.expanded_dockerfile_content().await?;
                    return image_from_dockerfile(
                        dockerfile_contents,
                        &main_service.build.as_ref().and_then(|b| b.target.clone()),
                    )
                    .ok_or_else(|| {
                        log::error!("Unable to find base image in Dockerfile");
                        DevContainerError::DevContainerParseFailed
                    });
                }
                if let Some(image) = &main_service.image {
                    return Ok(image.to_string());
                }

                log::error!("No valid base image found in docker-compose configuration");
                return Err(DevContainerError::DevContainerParseFailed);
            }
            DevContainerBuildType::None => {
                log::error!("Not a valid devcontainer config for build");
                return Err(DevContainerError::NotInValidProject);
            }
        }
    }

    async fn copy_local_feature(
        &self,
        feature_ref: &str,
        destination: &Path,
    ) -> Result<(), DevContainerError> {
        let source_path = normalize_path(&self.config_directory.join(feature_ref));

        if !self.fs.is_dir(&source_path).await {
            log::error!(
                "Local feature directory '{}' not found at {:?}",
                feature_ref,
                source_path
            );
            return Err(DevContainerError::ResourceFetchFailed);
        }

        let items = fs::read_dir_items(&*self.fs, &source_path)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to read local feature directory {:?}: {e}",
                    source_path
                );
                DevContainerError::FilesystemError
            })?;

        for (item_path, is_dir) in &items {
            let relative = item_path.strip_prefix(&source_path).map_err(|e| {
                log::error!("Failed to compute relative path for {:?}: {e}", item_path);
                DevContainerError::FilesystemError
            })?;
            let dest_path = destination.join(relative);

            if *is_dir {
                self.fs.create_dir(&dest_path).await.map_err(|e| {
                    log::error!("Failed to create directory {:?}: {e}", dest_path);
                    DevContainerError::FilesystemError
                })?;
            } else {
                let content = self.fs.load_bytes(item_path).await.map_err(|e| {
                    log::error!("Failed to read file {:?}: {e}", item_path);
                    DevContainerError::FilesystemError
                })?;
                self.fs.write(&dest_path, &content).await.map_err(|e| {
                    log::error!("Failed to write file {:?}: {e}", dest_path);
                    DevContainerError::FilesystemError
                })?;
            }
        }

        Ok(())
    }

    async fn download_feature_and_dockerfile_resources(&mut self) -> Result<(), DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet download resources"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };
        let root_image_tag = self.get_base_image_from_config().await?;
        let root_image = self.docker_client.inspect(&root_image_tag).await?;

        let temp_base = std::env::temp_dir().join("devcontainer-zed");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let features_content_dir = temp_base.join(format!("container-features-{}", timestamp));
        let empty_context_dir = temp_base.join("empty-folder");

        self.fs
            .create_dir(&features_content_dir)
            .await
            .map_err(|e| {
                log::error!("Failed to create features content dir: {e}");
                DevContainerError::FilesystemError
            })?;

        self.fs.create_dir(&empty_context_dir).await.map_err(|e| {
            log::error!("Failed to create empty context dir: {e}");
            DevContainerError::FilesystemError
        })?;

        let dockerfile_path = features_content_dir.join("Dockerfile.extended");
        let image_tag =
            self.generate_features_image_tag(dockerfile_path.clone().display().to_string());

        let build_info = FeaturesBuildInfo {
            dockerfile_path,
            features_content_dir,
            empty_context_dir,
            build_image: dev_container.image.clone(),
            image_tag,
        };

        let features = match &dev_container.features {
            Some(features) => features,
            None => &HashMap::new(),
        };

        let container_user = get_container_user_from_config(&root_image, self)?;
        let remote_user = get_remote_user_from_config(&root_image, self)?;

        let builtin_env_content = format!(
            "_CONTAINER_USER={}\n_REMOTE_USER={}\n",
            container_user, remote_user
        );

        let builtin_env_path = build_info
            .features_content_dir
            .join("devcontainer-features.builtin.env");

        self.fs
            .write(&builtin_env_path, &builtin_env_content.as_bytes())
            .await
            .map_err(|e| {
                log::error!("Failed to write builtin env file: {e}");
                DevContainerError::FilesystemError
            })?;

        let ordered_features =
            resolve_feature_order(features, &dev_container.override_feature_install_order);

        for (index, (feature_ref, options)) in ordered_features.iter().enumerate() {
            if matches!(options, FeatureOptions::Bool(false)) {
                log::debug!(
                    "Feature '{}' is disabled (set to false), skipping",
                    feature_ref
                );
                continue;
            }

            let feature_id = extract_feature_id(feature_ref);
            let consecutive_id = format!("{}_{}", feature_id, index);
            let feature_dir = build_info.features_content_dir.join(&consecutive_id);

            self.fs.create_dir(&feature_dir).await.map_err(|e| {
                log::error!(
                    "Failed to create feature directory for {}: {e}",
                    feature_ref
                );
                DevContainerError::FilesystemError
            })?;

            if is_local_feature_ref(feature_ref) {
                self.copy_local_feature(feature_ref, &feature_dir).await?;
            } else {
                let oci_ref = parse_oci_feature_ref(feature_ref).ok_or_else(|| {
                    log::error!(
                        "Feature '{}' is not a supported OCI feature reference",
                        feature_ref
                    );
                    DevContainerError::DevContainerParseFailed
                })?;
                let TokenResponse { token } =
                    get_oci_token(&oci_ref.registry, &oci_ref.path, &self.http_client)
                        .await
                        .map_err(|e| {
                            log::error!(
                                "Failed to get OCI token for feature '{}': {e}",
                                feature_ref
                            );
                            DevContainerError::ResourceFetchFailed
                        })?;
                let manifest = get_oci_manifest(
                    &oci_ref.registry,
                    &oci_ref.path,
                    &token,
                    &self.http_client,
                    &oci_ref.version,
                    None,
                )
                .await
                .map_err(|e| {
                    log::error!(
                        "Failed to fetch OCI manifest for feature '{}': {e}",
                        feature_ref
                    );
                    DevContainerError::ResourceFetchFailed
                })?;
                let digest = &manifest
                    .layers
                    .first()
                    .ok_or_else(|| {
                        log::error!(
                            "OCI manifest for feature '{}' contains no layers",
                            feature_ref
                        );
                        DevContainerError::ResourceFetchFailed
                    })?
                    .digest;
                download_oci_tarball(
                    &token,
                    &oci_ref.registry,
                    &oci_ref.path,
                    digest,
                    "application/vnd.devcontainers.layer.v1+tar",
                    &feature_dir,
                    &self.http_client,
                    &self.fs,
                    None,
                )
                .await?;
            }

            let feature_json_path = &feature_dir.join("devcontainer-feature.json");
            if !self.fs.is_file(feature_json_path).await {
                let message = format!(
                    "No devcontainer-feature.json found in {:?}, no defaults to apply",
                    feature_json_path
                );
                log::error!("{message}");
                return Err(DevContainerError::ResourceFetchFailed);
            }

            let contents = self.fs.load(&feature_json_path).await.map_err(|e| {
                log::error!("error reading devcontainer-feature.json: {:?}", e);
                DevContainerError::FilesystemError
            })?;

            let contents_parsed = self.parse_nonremote_vars_for_content(&contents)?;

            let feature_json: DevContainerFeatureJson =
                serde_json_lenient::from_value(contents_parsed).map_err(|e| {
                    log::error!("Failed to parse devcontainer-feature.json: {e}");
                    DevContainerError::ResourceFetchFailed
                })?;

            let feature_manifest = FeatureManifest::new(
                consecutive_id,
                feature_ref.to_string(),
                feature_dir,
                feature_json,
            );

            log::debug!("Prepared feature content for '{}'", feature_ref);

            let env_content = feature_manifest
                .write_feature_env(&self.fs, options)
                .await?;

            let wrapper_content = generate_install_wrapper(feature_ref, feature_id, &env_content)?;

            self.fs
                .write(
                    &feature_manifest
                        .file_path()
                        .join("devcontainer-features-install.sh"),
                    &wrapper_content.as_bytes(),
                )
                .await
                .map_err(|e| {
                    log::error!("Failed to write install wrapper for {}: {e}", feature_ref);
                    DevContainerError::FilesystemError
                })?;

            self.features.push(feature_manifest);
        }

        // --- Phase 3: Generate extended Dockerfile from the inflated manifests ---

        let is_compose = match dev_container.build_type() {
            DevContainerBuildType::DockerCompose => true,
            _ => false,
        };
        let use_buildkit = self.docker_client.supports_compose_buildkit() || !is_compose;

        let dockerfile_base_content = if let Some(location) = &self.dockerfile_location().await {
            self.fs.load(location).await.log_err()
        } else {
            None
        };

        let build_target = if is_compose {
            find_primary_service(&self.docker_compose_manifest().await?, self)?
                .1
                .build
                .and_then(|b| b.target)
        } else {
            dev_container.build.as_ref().and_then(|b| b.target.clone())
        };

        let dockerfile_content = dockerfile_base_content
            .map(|content| {
                dockerfile_inject_alias(
                    &content,
                    "dev_container_auto_added_stage_label",
                    build_target,
                )
            })
            .unwrap_or_default();

        let dockerfile_content = self.generate_dockerfile_extended(
            &container_user,
            &remote_user,
            dockerfile_content,
            use_buildkit,
        );

        self.fs
            .write(&build_info.dockerfile_path, &dockerfile_content.as_bytes())
            .await
            .map_err(|e| {
                log::error!("Failed to write Dockerfile.extended: {e}");
                DevContainerError::FilesystemError
            })?;

        log::debug!(
            "Features build resources written to {:?}",
            build_info.features_content_dir
        );

        self.root_image = Some(root_image);
        self.features_build_info = Some(build_info);

        Ok(())
    }

    fn generate_dockerfile_extended(
        &self,
        container_user: &str,
        remote_user: &str,
        dockerfile_content: String,
        use_buildkit: bool,
    ) -> String {
        let update_remote_user_uid = self.dev_container().update_remote_user_uid.unwrap_or(true);

        let feature_layers: String = self
            .features
            .iter()
            .map(|manifest| {
                manifest.generate_dockerfile_feature_layer(
                    use_buildkit,
                    FEATURES_CONTAINER_TEMP_DEST_FOLDER,
                )
            })
            .collect();

        let container_home_cmd = get_ent_passwd_shell_command(container_user);
        let remote_home_cmd = get_ent_passwd_shell_command(remote_user);

        let dest = FEATURES_CONTAINER_TEMP_DEST_FOLDER;

        let feature_content_source_stage = if use_buildkit {
            "".to_string()
        } else {
            "\nFROM dev_container_feature_content_temp as dev_containers_feature_content_source\n"
                .to_string()
        };

        let builtin_env_source_path = if use_buildkit {
            "./devcontainer-features.builtin.env"
        } else {
            "/tmp/build-features/devcontainer-features.builtin.env"
        };

        let mut extended_dockerfile = format!(
            r#"ARG _DEV_CONTAINERS_BASE_IMAGE=placeholder

{dockerfile_content}
{feature_content_source_stage}
FROM $_DEV_CONTAINERS_BASE_IMAGE AS dev_containers_feature_content_normalize
USER root
COPY --from=dev_containers_feature_content_source {builtin_env_source_path} /tmp/build-features/
RUN chmod -R 0755 /tmp/build-features/

FROM $_DEV_CONTAINERS_BASE_IMAGE AS dev_containers_target_stage

USER root

RUN mkdir -p {dest}
COPY --from=dev_containers_feature_content_normalize /tmp/build-features/ {dest}

RUN \
echo "_CONTAINER_USER_HOME=$({container_home_cmd} | cut -d: -f6)" >> {dest}/devcontainer-features.builtin.env && \
echo "_REMOTE_USER_HOME=$({remote_home_cmd} | cut -d: -f6)" >> {dest}/devcontainer-features.builtin.env

{feature_layers}

ARG _DEV_CONTAINERS_IMAGE_USER=root
USER $_DEV_CONTAINERS_IMAGE_USER
"#
        );

        // If we're not adding a uid update layer, then we should add env vars to this layer instead
        if !update_remote_user_uid {
            extended_dockerfile = format!(
                r#"{extended_dockerfile}
# Ensure that /etc/profile does not clobber the existing path
RUN sed -i -E 's/((^|\s)PATH=)([^\$]*)$/\1\${{PATH:-\3}}/g' /etc/profile || true
"#
            );

            for feature in &self.features {
                let container_env_layer = feature.generate_dockerfile_env();
                extended_dockerfile = format!("{extended_dockerfile}\n{container_env_layer}");
            }

            if let Some(env) = &self.dev_container().container_env {
                for (key, value) in env {
                    extended_dockerfile = format!("{extended_dockerfile}ENV {key}={value}\n");
                }
            }
        }

        extended_dockerfile
    }

    fn build_merged_resources(
        &self,
        base_image: DockerInspect,
        image_tag: &str,
    ) -> Result<DockerBuildResources, DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet merge resources"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };
        let mut mounts = Vec::new();
        let mut cap_add = Vec::new();
        let mut security_opt = Vec::new();
        let mut privileged = false;
        let mut init = false;
        let mut metadata_entries = base_image
            .config
            .labels
            .metadata
            .clone()
            .unwrap_or_default();
        for entry in &mut metadata_entries {
            for value in entry.values_mut() {
                self.substitute_nonremote_vars_in_value(value)?;
            }
        }

        for entry in &metadata_entries {
            if let Some(serde_json_lenient::Value::Array(metadata_mounts)) = entry.get("mounts") {
                for mount in metadata_mounts {
                    if let Some(mount) = mount_definition_from_metadata(mount) {
                        append_mount_with_target_override(&mut mounts, mount);
                    }
                }
            }
            if entry.get("privileged").and_then(|value| value.as_bool()) == Some(true) {
                privileged = true;
            }
            if entry.get("init").and_then(|value| value.as_bool()) == Some(true) {
                init = true;
            }
            if let Some(serde_json_lenient::Value::Array(caps)) = entry.get("capAdd") {
                for cap in caps.iter().filter_map(|cap| cap.as_str()) {
                    push_unique_string(&mut cap_add, cap);
                }
            }
            if let Some(serde_json_lenient::Value::Array(opts)) = entry.get("securityOpt") {
                for opt in opts.iter().filter_map(|opt| opt.as_str()) {
                    push_unique_string(&mut security_opt, opt);
                }
            }
        }

        for feature in &self.features {
            for mount in feature.mounts() {
                append_mount_with_target_override(&mut mounts, mount);
            }
            if feature.privileged() {
                privileged = true;
            }
            if feature.init() {
                init = true;
            }
            for cap in feature.cap_add() {
                push_unique_string(&mut cap_add, &cap);
            }
            for opt in feature.security_opt() {
                push_unique_string(&mut security_opt, &opt);
            }
        }

        for mount in dev_container.mounts.clone().unwrap_or_default() {
            append_mount_with_target_override(&mut mounts, mount);
        }
        privileged |= dev_container.privileged.unwrap_or(false);
        init |= dev_container.init.unwrap_or(false);
        for cap in dev_container.cap_add.clone().unwrap_or_default() {
            push_unique_string(&mut cap_add, &cap);
        }
        for opt in dev_container.security_opt.clone().unwrap_or_default() {
            push_unique_string(&mut security_opt, &opt);
        }

        let entrypoint_script = if dev_container.override_command() {
            let mut entrypoint_script_lines = vec![
                "echo Container started".to_string(),
                "trap \"exit 0\" 15".to_string(),
            ];

            for entrypoint in self.features.iter().filter_map(|f| f.entrypoint()) {
                entrypoint_script_lines.push(entrypoint.clone());
            }
            entrypoint_script_lines.append(&mut vec![
                "exec \"$@\"".to_string(),
                "while sleep 1 & wait $!; do :; done".to_string(),
            ]);

            Some(entrypoint_script_lines.join("\n").trim().to_string())
        } else {
            None
        };

        let mut container_env = HashMap::new();
        for entry in &metadata_entries {
            if let Some(serde_json_lenient::Value::Object(env_map)) = entry.get("containerEnv") {
                for (k, v) in env_map {
                    if let Some(s) = v.as_str() {
                        container_env.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        if let Some(config_env) = &dev_container.container_env {
            for (k, v) in config_env {
                container_env.insert(k.clone(), v.clone());
            }
        }

        Ok(DockerBuildResources {
            image: base_image,
            image_tag: image_tag.to_string(),
            additional_mounts: mounts,
            container_env,
            privileged,
            init,
            cap_add,
            security_opt,
            entrypoint_script,
        })
    }

    async fn build_resources(&self) -> Result<DevContainerBuildResources, DevContainerError> {
        if let ConfigStatus::Deserialized(_) = &self.config {
            log::error!(
                "Dev container has not yet been parsed for variable expansion. Cannot yet build resources"
            );
            return Err(DevContainerError::DevContainerParseFailed);
        }
        let dev_container = self.dev_container();
        match dev_container.build_type() {
            DevContainerBuildType::Image(base_image) => {
                let built_docker_image = self.build_docker_image().await?;
                let has_features = dev_container
                    .features
                    .as_ref()
                    .is_some_and(|features| !features.is_empty());
                let features_image_tag = self
                    .features_build_info
                    .as_ref()
                    .map(|info| info.image_tag.as_str());
                let uid_base_image = if has_features {
                    features_image_tag.unwrap_or(&base_image)
                } else {
                    &base_image
                };
                let will_update_remote_user_uid =
                    self.should_update_remote_user_uid(&built_docker_image)?;

                let built_docker_image = self
                    .update_remote_user_uid(built_docker_image, uid_base_image)
                    .await?;

                let image_tag = if has_features || will_update_remote_user_uid {
                    features_image_tag.unwrap_or(&base_image)
                } else {
                    &base_image
                };
                let resources = self.build_merged_resources(built_docker_image, image_tag)?;
                Ok(DevContainerBuildResources::Docker(resources))
            }
            DevContainerBuildType::Dockerfile(_) => {
                let built_docker_image = self.build_docker_image().await?;
                let Some(features_build_info) = &self.features_build_info else {
                    log::error!(
                        "Can't attempt to build update UID dockerfile before initial docker build"
                    );
                    return Err(DevContainerError::DevContainerParseFailed);
                };
                let built_docker_image = self
                    .update_remote_user_uid(built_docker_image, &features_build_info.image_tag)
                    .await?;

                let resources = self
                    .build_merged_resources(built_docker_image, &features_build_info.image_tag)?;
                Ok(DevContainerBuildResources::Docker(resources))
            }
            DevContainerBuildType::DockerCompose => {
                log::debug!("Using docker compose. Building extended compose files");
                let docker_compose_resources = self.build_and_extend_compose_files().await?;

                return Ok(DevContainerBuildResources::DockerCompose(
                    docker_compose_resources,
                ));
            }
            DevContainerBuildType::None => {
                return Err(DevContainerError::DevContainerParseFailed);
            }
        }
    }

    async fn run_dev_container(
        &self,
        build_resources: DevContainerBuildResources,
    ) -> Result<DevContainerUp, DevContainerError> {
        let ConfigStatus::VariableParsed(_) = &self.config else {
            log::error!(
                "Variables have not been parsed; cannot proceed with running the dev container"
            );
            return Err(DevContainerError::DevContainerParseFailed);
        };
        let running_container = match build_resources {
            DevContainerBuildResources::DockerCompose(resources) => {
                self.run_docker_compose(resources).await?
            }
            DevContainerBuildResources::Docker(resources) => {
                self.run_docker_image(resources).await?
            }
        };

        let remote_user = get_remote_user_from_config(&running_container, self)?;
        let remote_workspace_folder = self.remote_workspace_folder()?;

        let remote_env = self.runtime_remote_env(&running_container.config.env_as_map()?)?;

        Ok(DevContainerUp {
            started_at: running_container
                .state
                .as_ref()
                .and_then(|state| state.started_at.clone()),
            container_id: running_container.id,
            remote_user,
            remote_workspace_folder: remote_workspace_folder.display().to_string(),
            extension_ids: self.extension_ids(),
            remote_env,
        })
    }

    async fn docker_compose_manifest(&self) -> Result<DockerComposeResources, DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet get docker compose files"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };
        let Some(docker_compose_files) = dev_container.docker_compose_file.clone() else {
            return Err(DevContainerError::DevContainerParseFailed);
        };
        // Normalize upfront so every downstream consumer of
        // `DockerComposeResources.files` (compose fragment reads, project-name
        // derivation, `docker compose -f` invocations, …) sees resolved paths.
        // `dockerComposeFile` entries are joined verbatim with
        // `config_directory`, so raw entries can carry `..` components.
        let docker_compose_full_paths = docker_compose_files
            .iter()
            .map(|relative| normalize_path(&self.config_directory.join(relative)))
            .collect::<Vec<PathBuf>>();

        let Some(config) = self
            .docker_client
            .get_docker_compose_config(&docker_compose_full_paths)
            .await?
        else {
            log::error!("Output could not deserialize into DockerComposeConfig");
            return Err(DevContainerError::DevContainerParseFailed);
        };
        Ok(DockerComposeResources {
            files: docker_compose_full_paths,
            config,
        })
    }

    async fn build_and_extend_compose_files(
        &self,
    ) -> Result<DockerComposeResources, DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet build from compose files"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };

        let Some(features_build_info) = &self.features_build_info else {
            log::error!(
                "Cannot build and extend compose files: features build info is not yet constructed"
            );
            return Err(DevContainerError::DevContainerParseFailed);
        };
        let mut docker_compose_resources = self.docker_compose_manifest().await?;
        let supports_buildkit = self.docker_client.supports_compose_buildkit();

        let (main_service_name, main_service) =
            find_primary_service(&docker_compose_resources, self)?;
        let (built_service_image, built_service_image_tag) = if main_service
            .build
            .as_ref()
            .map(|b| b.dockerfile.as_ref())
            .is_some()
        {
            if !supports_buildkit {
                self.build_feature_content_image().await?;
            }

            let dockerfile_path = &features_build_info.dockerfile_path;

            let build_args = if !supports_buildkit {
                HashMap::from([
                    (
                        "_DEV_CONTAINERS_BASE_IMAGE".to_string(),
                        "dev_container_auto_added_stage_label".to_string(),
                    ),
                    ("_DEV_CONTAINERS_IMAGE_USER".to_string(), "root".to_string()),
                ])
            } else {
                HashMap::from([
                    ("BUILDKIT_INLINE_CACHE".to_string(), "1".to_string()),
                    (
                        "_DEV_CONTAINERS_BASE_IMAGE".to_string(),
                        "dev_container_auto_added_stage_label".to_string(),
                    ),
                    ("_DEV_CONTAINERS_IMAGE_USER".to_string(), "root".to_string()),
                ])
            };

            let additional_contexts = if !supports_buildkit {
                None
            } else {
                Some(HashMap::from([(
                    "dev_containers_feature_content_source".to_string(),
                    features_build_info
                        .features_content_dir
                        .display()
                        .to_string(),
                )]))
            };

            let build_override = DockerComposeConfig {
                name: None,
                services: HashMap::from([(
                    main_service_name.clone(),
                    DockerComposeService {
                        image: Some(features_build_info.image_tag.clone()),
                        entrypoint: None,
                        cap_add: None,
                        security_opt: None,
                        labels: None,
                        build: Some(DockerComposeServiceBuild {
                            context: Some(
                                main_service
                                    .build
                                    .as_ref()
                                    .and_then(|b| b.context.clone())
                                    .unwrap_or_else(|| {
                                        features_build_info.empty_context_dir.display().to_string()
                                    }),
                            ),
                            dockerfile: Some(dockerfile_path.display().to_string()),
                            target: Some("dev_containers_target_stage".to_string()),
                            args: Some(build_args),
                            additional_contexts,
                        }),
                        volumes: Vec::new(),
                        ..Default::default()
                    },
                )]),
                volumes: HashMap::new(),
            };

            let temp_base = std::env::temp_dir().join("devcontainer-zed");
            let config_location = temp_base.join("docker_compose_build.json");

            let config_json = serde_json_lenient::to_string(&build_override).map_err(|e| {
                log::error!("Error serializing docker compose runtime override: {e}");
                DevContainerError::DevContainerParseFailed
            })?;

            self.fs
                .write(&config_location, config_json.as_bytes())
                .await
                .map_err(|e| {
                    log::error!("Error writing the runtime override file: {e}");
                    DevContainerError::FilesystemError
                })?;

            docker_compose_resources.files.push(config_location);

            let project_name = self.project_name().await?;
            let compose_services =
                compose_service_list(&main_service_name, dev_container.run_services.as_ref());
            self.docker_client
                .docker_compose_build(
                    &docker_compose_resources.files,
                    &project_name,
                    compose_services.as_ref(),
                )
                .await?;
            (
                self.docker_client
                    .inspect(&features_build_info.image_tag)
                    .await?,
                &features_build_info.image_tag,
            )
        } else if let Some(image) = &main_service.image {
            if dev_container
                .features
                .as_ref()
                .is_none_or(|features| features.is_empty())
            {
                (self.docker_client.inspect(image).await?, image)
            } else {
                if !supports_buildkit {
                    self.build_feature_content_image().await?;
                }

                let dockerfile_path = &features_build_info.dockerfile_path;

                let build_args = if !supports_buildkit {
                    HashMap::from([
                        ("_DEV_CONTAINERS_BASE_IMAGE".to_string(), image.clone()),
                        ("_DEV_CONTAINERS_IMAGE_USER".to_string(), "root".to_string()),
                    ])
                } else {
                    HashMap::from([
                        ("BUILDKIT_INLINE_CACHE".to_string(), "1".to_string()),
                        ("_DEV_CONTAINERS_BASE_IMAGE".to_string(), image.clone()),
                        ("_DEV_CONTAINERS_IMAGE_USER".to_string(), "root".to_string()),
                    ])
                };

                let additional_contexts = if !supports_buildkit {
                    None
                } else {
                    Some(HashMap::from([(
                        "dev_containers_feature_content_source".to_string(),
                        features_build_info
                            .features_content_dir
                            .display()
                            .to_string(),
                    )]))
                };

                let build_override = DockerComposeConfig {
                    name: None,
                    services: HashMap::from([(
                        main_service_name.clone(),
                        DockerComposeService {
                            image: Some(features_build_info.image_tag.clone()),
                            entrypoint: None,
                            cap_add: None,
                            security_opt: None,
                            labels: None,
                            build: Some(DockerComposeServiceBuild {
                                context: Some(
                                    features_build_info.empty_context_dir.display().to_string(),
                                ),
                                dockerfile: Some(dockerfile_path.display().to_string()),
                                target: Some("dev_containers_target_stage".to_string()),
                                args: Some(build_args),
                                additional_contexts,
                            }),
                            volumes: Vec::new(),
                            ..Default::default()
                        },
                    )]),
                    volumes: HashMap::new(),
                };

                let temp_base = std::env::temp_dir().join("devcontainer-zed");
                let config_location = temp_base.join("docker_compose_build.json");

                let config_json = serde_json_lenient::to_string(&build_override).map_err(|e| {
                    log::error!("Error serializing docker compose runtime override: {e}");
                    DevContainerError::DevContainerParseFailed
                })?;

                self.fs
                    .write(&config_location, config_json.as_bytes())
                    .await
                    .map_err(|e| {
                        log::error!("Error writing the runtime override file: {e}");
                        DevContainerError::FilesystemError
                    })?;

                docker_compose_resources.files.push(config_location);

                let project_name = self.project_name().await?;
                let compose_services =
                    compose_service_list(&main_service_name, dev_container.run_services.as_ref());
                self.docker_client
                    .docker_compose_build(
                        &docker_compose_resources.files,
                        &project_name,
                        compose_services.as_ref(),
                    )
                    .await?;

                (
                    self.docker_client
                        .inspect(&features_build_info.image_tag)
                        .await?,
                    &features_build_info.image_tag,
                )
            }
        } else {
            log::error!("Docker compose must have either image or dockerfile defined");
            return Err(DevContainerError::DevContainerParseFailed);
        };

        let built_service_image = self
            .update_remote_user_uid(built_service_image, built_service_image_tag)
            .await?;

        let resources =
            self.build_merged_resources(built_service_image, built_service_image_tag)?;

        let network_mode = main_service.network_mode.as_ref();
        let network_mode_service = network_mode.and_then(|mode| mode.strip_prefix("service:"));
        let runtime_override_file = self
            .write_runtime_override_file(&main_service_name, network_mode_service, resources)
            .await?;

        docker_compose_resources.files.push(runtime_override_file);

        Ok(docker_compose_resources)
    }

    async fn write_runtime_override_file(
        &self,
        main_service_name: &str,
        network_mode_service: Option<&str>,
        resources: DockerBuildResources,
    ) -> Result<PathBuf, DevContainerError> {
        let config =
            self.build_runtime_override(main_service_name, network_mode_service, resources)?;
        let temp_base = std::env::temp_dir().join("devcontainer-zed");
        let config_location = temp_base.join("docker_compose_runtime.json");

        let config_json = serde_json_lenient::to_string(&config).map_err(|e| {
            log::error!("Error serializing docker compose runtime override: {e}");
            DevContainerError::DevContainerParseFailed
        })?;

        self.fs
            .write(&config_location, config_json.as_bytes())
            .await
            .map_err(|e| {
                log::error!("Error writing the runtime override file: {e}");
                DevContainerError::FilesystemError
            })?;

        Ok(config_location)
    }

    fn build_runtime_override(
        &self,
        main_service_name: &str,
        network_mode_service: Option<&str>,
        resources: DockerBuildResources,
    ) -> Result<DockerComposeConfig, DevContainerError> {
        let mut runtime_labels = HashMap::new();

        {
            let mut metadata_entries: Vec<serde_json_lenient::Value> = Vec::new();

            if let Some(image_metadata) = &resources.image.config.labels.metadata {
                for entry in image_metadata {
                    if let Ok(val) = serde_json_lenient::to_value(entry) {
                        metadata_entries.push(val);
                    }
                }
            }

            for feature in &self.features {
                let entry = feature.build_metadata_entry();
                if !entry.is_empty() {
                    metadata_entries.push(serde_json_lenient::Value::Object(entry));
                }
            }

            if let Ok(substituted_json) = self.parse_nonremote_vars_for_content(&self.raw_config) {
                let config_entry = build_devcontainer_metadata_entry(&substituted_json);
                if !config_entry.is_empty() {
                    metadata_entries.push(serde_json_lenient::Value::Object(config_entry));
                }
            }

            if !metadata_entries.is_empty() {
                let serialized = serde_json_lenient::to_string(&metadata_entries).map_err(|e| {
                    log::error!("Error serializing docker image metadata: {e}");
                    DevContainerError::ContainerNotValid(resources.image.id.clone())
                })?;
                runtime_labels.insert(
                    "devcontainer.metadata".to_string(),
                    escape_compose_interpolation(&serialized),
                );
            }
        }

        for (k, v) in self.identifying_labels() {
            runtime_labels.insert(k.to_string(), escape_compose_interpolation(&v));
        }

        let config_volumes: HashMap<String, DockerComposeVolume> = resources
            .additional_mounts
            .iter()
            .filter_map(|mount| {
                if let Some(mount_type) = &mount.mount_type
                    && mount_type.to_lowercase() == "volume"
                    && let Some(source) = &mount.source
                {
                    Some((
                        source.clone(),
                        DockerComposeVolume {
                            name: Some(source.clone()),
                        },
                    ))
                } else {
                    None
                }
            })
            .collect();

        let volumes: Vec<MountDefinition> = resources
            .additional_mounts
            .iter()
            .map(|v| MountDefinition {
                source: v.source.clone(),
                target: v.target.clone(),
                mount_type: v.mount_type.clone(),
            })
            .collect();

        let entrypoint = resources.entrypoint_script.map(|script| {
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                script,
                "-".to_string(),
            ]
        });

        let cap_add = if resources.cap_add.is_empty() {
            None
        } else {
            Some(resources.cap_add)
        };
        let security_opt = if resources.security_opt.is_empty() {
            None
        } else {
            Some(resources.security_opt)
        };

        let environment = if resources.container_env.is_empty() {
            None
        } else {
            Some(
                resources
                    .container_env
                    .into_iter()
                    .map(|(key, value)| (key, escape_compose_interpolation(&value)))
                    .collect(),
            )
        };
        let privileged = resources.privileged.then_some(true);
        let init = resources.init.then_some(true);

        let mut main_service = DockerComposeService {
            entrypoint,
            cap_add,
            security_opt,
            labels: Some(runtime_labels),
            volumes,
            privileged,
            init,
            environment,
            ..Default::default()
        };
        // let mut extra_service_port_declarations: Vec<(String, DockerComposeService)> = Vec::new();
        let mut service_declarations: HashMap<String, DockerComposeService> = HashMap::new();
        if let Some(forward_ports) = &self.dev_container().forward_ports {
            let main_service_ports: Vec<String> = forward_ports
                .iter()
                .filter_map(|f| match f {
                    ForwardPort::Number(port) => Some(port.to_string()),
                    ForwardPort::String(port) => {
                        let parts: Vec<&str> = port.split(":").collect();
                        if parts.len() <= 1 {
                            Some(port.to_string())
                        } else if parts.len() == 2 {
                            if parts[0] == main_service_name {
                                Some(parts[1].to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                })
                .collect();
            for port in main_service_ports {
                // If the main service uses a different service's network bridge, append to that service's ports instead
                if let Some(network_service_name) = network_mode_service {
                    if let Some(service) = service_declarations.get_mut(network_service_name) {
                        service.ports.push(DockerComposeServicePort {
                            target: port.clone(),
                            published: port.clone(),
                            ..Default::default()
                        });
                    } else {
                        service_declarations.insert(
                            network_service_name.to_string(),
                            DockerComposeService {
                                ports: vec![DockerComposeServicePort {
                                    target: port.clone(),
                                    published: port.clone(),
                                    ..Default::default()
                                }],
                                ..Default::default()
                            },
                        );
                    }
                } else {
                    main_service.ports.push(DockerComposeServicePort {
                        target: port.clone(),
                        published: port.clone(),
                        ..Default::default()
                    });
                }
            }
            let other_service_ports: Vec<(&str, &str)> = forward_ports
                .iter()
                .filter_map(|f| match f {
                    ForwardPort::Number(_) => None,
                    ForwardPort::String(port) => {
                        let parts: Vec<&str> = port.split(":").collect();
                        if parts.len() != 2 {
                            None
                        } else {
                            if parts[0] == main_service_name {
                                None
                            } else {
                                Some((parts[0], parts[1]))
                            }
                        }
                    }
                })
                .collect();
            for (service_name, port) in other_service_ports {
                if let Some(service) = service_declarations.get_mut(service_name) {
                    service.ports.push(DockerComposeServicePort {
                        target: port.to_string(),
                        published: port.to_string(),
                        ..Default::default()
                    });
                } else {
                    service_declarations.insert(
                        service_name.to_string(),
                        DockerComposeService {
                            ports: vec![DockerComposeServicePort {
                                target: port.to_string(),
                                published: port.to_string(),
                                ..Default::default()
                            }],
                            ..Default::default()
                        },
                    );
                }
            }
        }

        service_declarations.insert(main_service_name.to_string(), main_service);
        let new_docker_compose_config = DockerComposeConfig {
            name: None,
            services: service_declarations,
            volumes: config_volumes,
        };

        Ok(new_docker_compose_config)
    }

    async fn build_docker_image(&self) -> Result<DockerInspect, DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet build image"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };

        match dev_container.build_type() {
            DevContainerBuildType::Image(image_tag) => {
                let base_image = self.docker_client.inspect(&image_tag).await?;
                if dev_container
                    .features
                    .as_ref()
                    .is_none_or(|features| features.is_empty())
                {
                    log::debug!("No features to add. Using base image");
                    return Ok(base_image);
                }
            }
            DevContainerBuildType::Dockerfile(_) => {}
            DevContainerBuildType::DockerCompose | DevContainerBuildType::None => {
                return Err(DevContainerError::DevContainerParseFailed);
            }
        };

        let mut command = self.create_docker_build()?;

        let output = self
            .command_runner
            .run_command(&mut command)
            .await
            .map_err(|e| {
                log::error!("Error building docker image: {e}");
                DevContainerError::CommandFailed(command.get_program().display().to_string())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("docker buildx build failed: {stderr}");
            return Err(DevContainerError::CommandFailed(
                command.get_program().display().to_string(),
            ));
        }

        // After a successful build, inspect the newly tagged image to get its metadata
        let Some(features_build_info) = &self.features_build_info else {
            log::error!("Features build info expected, but not created");
            return Err(DevContainerError::DevContainerParseFailed);
        };
        let image = self
            .docker_client
            .inspect(&features_build_info.image_tag)
            .await?;

        Ok(image)
    }

    fn should_update_remote_user_uid(
        &self,
        image: &DockerInspect,
    ) -> Result<bool, DevContainerError> {
        if self.features_build_info.is_none() {
            return Ok(false);
        }
        if self.dev_container().update_remote_user_uid == Some(false) {
            return Ok(false);
        }
        let remote_user = get_remote_user_from_config(image, self)?;
        Ok(remote_user != "root" && !remote_user.chars().all(|c| c.is_ascii_digit()))
    }

    async fn update_remote_user_uid(
        &self,
        image: DockerInspect,
        base_image: &str,
    ) -> Result<DockerInspect, DevContainerError> {
        let Some(features_build_info) = &self.features_build_info else {
            return Ok(image);
        };

        if !self.should_update_remote_user_uid(&image)? {
            return Ok(image);
        }

        let remote_user = get_remote_user_from_config(&image, self)?;

        let image_user = image
            .config
            .image_user
            .as_deref()
            .unwrap_or("root")
            .to_string();

        let host_uid = Command::new("id")
            .arg("-u")
            .output()
            .await
            .map_err(|e| {
                log::error!("Failed to get host UID: {e}");
                DevContainerError::CommandFailed("id -u".to_string())
            })
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .map_err(|e| {
                        log::error!("Failed to parse host UID: {e}");
                        DevContainerError::CommandFailed("id -u".to_string())
                    })
            })?;

        let host_gid = Command::new("id")
            .arg("-g")
            .output()
            .await
            .map_err(|e| {
                log::error!("Failed to get host GID: {e}");
                DevContainerError::CommandFailed("id -g".to_string())
            })
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .map_err(|e| {
                        log::error!("Failed to parse host GID: {e}");
                        DevContainerError::CommandFailed("id -g".to_string())
                    })
            })?;

        let dockerfile_content = self.generate_update_uid_dockerfile();

        let dockerfile_path = features_build_info
            .features_content_dir
            .join("updateUID.Dockerfile");
        self.fs
            .write(&dockerfile_path, dockerfile_content.as_bytes())
            .await
            .map_err(|e| {
                log::error!("Failed to write updateUID Dockerfile: {e}");
                DevContainerError::FilesystemError
            })?;

        let updated_image_tag = features_build_info.image_tag.clone();

        let mut command = Command::new(self.docker_client.docker_cli());
        // Without a usable BuildKit, force the classic builder: the build's
        // `FROM $BASE_IMAGE` references the locally-built features image, which
        // only resolves from the daemon's image store under the classic builder.
        if !self.docker_client.supports_compose_buildkit()
            && self.docker_client.docker_cli() != "podman"
        {
            command.env("DOCKER_BUILDKIT", "0");
        }
        command.args(["build"]);
        command.args(["-f", &dockerfile_path.display().to_string()]);
        command.args(["-t", &updated_image_tag]);
        command.args(["--build-arg", &format!("BASE_IMAGE={}", base_image)]);
        command.args(["--build-arg", &format!("REMOTE_USER={}", remote_user)]);
        command.args(["--build-arg", &format!("NEW_UID={}", host_uid)]);
        command.args(["--build-arg", &format!("NEW_GID={}", host_gid)]);
        command.args(["--build-arg", &format!("IMAGE_USER={}", image_user)]);
        command.arg(features_build_info.empty_context_dir.display().to_string());

        let output = self
            .command_runner
            .run_command(&mut command)
            .await
            .map_err(|e| {
                log::error!("Error building UID update image: {e}");
                DevContainerError::CommandFailed(command.get_program().display().to_string())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("UID update build failed: {stderr}");
            return Err(DevContainerError::CommandFailed(
                command.get_program().display().to_string(),
            ));
        }

        self.docker_client.inspect(&updated_image_tag).await
    }

    fn generate_update_uid_dockerfile(&self) -> String {
        let mut dockerfile = r#"ARG BASE_IMAGE
FROM $BASE_IMAGE

USER root

ARG REMOTE_USER
ARG NEW_UID
ARG NEW_GID
SHELL ["/bin/sh", "-c"]
RUN eval $(sed -n "s/${REMOTE_USER}:[^:]*:\([^:]*\):\([^:]*\):[^:]*:\([^:]*\).*/OLD_UID=\1;OLD_GID=\2;HOME_FOLDER=\3/p" /etc/passwd); \
	eval $(sed -n "s/\([^:]*\):[^:]*:${NEW_UID}:.*/EXISTING_USER=\1/p" /etc/passwd); \
	eval $(sed -n "s/\([^:]*\):[^:]*:${NEW_GID}:.*/EXISTING_GROUP=\1/p" /etc/group); \
	if [ -z "$OLD_UID" ]; then \
		echo "Remote user not found in /etc/passwd ($REMOTE_USER)."; \
	elif [ "$OLD_UID" = "$NEW_UID" -a "$OLD_GID" = "$NEW_GID" ]; then \
		echo "UIDs and GIDs are the same ($NEW_UID:$NEW_GID)."; \
	elif [ "$OLD_UID" != "$NEW_UID" -a -n "$EXISTING_USER" ]; then \
		echo "User with UID exists ($EXISTING_USER=$NEW_UID)."; \
	else \
		if [ "$OLD_GID" != "$NEW_GID" -a -n "$EXISTING_GROUP" ]; then \
			FREE_GID=65532; \
			while grep -q ":[^:]*:${FREE_GID}:" /etc/group; do FREE_GID=$((FREE_GID - 1)); done; \
			echo "Reassigning group $EXISTING_GROUP from GID $NEW_GID to $FREE_GID."; \
			sed -i -e "s/\(${EXISTING_GROUP}:[^:]*:\)${NEW_GID}:/\1${FREE_GID}:/" /etc/group; \
		fi; \
		echo "Updating UID:GID from $OLD_UID:$OLD_GID to $NEW_UID:$NEW_GID."; \
		sed -i -e "s/\(${REMOTE_USER}:[^:]*:\)[^:]*:[^:]*/\1${NEW_UID}:${NEW_GID}/" /etc/passwd; \
		if [ "$OLD_GID" != "$NEW_GID" ]; then \
			sed -i -e "s/\([^:]*:[^:]*:\)${OLD_GID}:/\1${NEW_GID}:/" /etc/group; \
		fi; \
		chown -R $NEW_UID:$NEW_GID $HOME_FOLDER; \
	fi;

ARG IMAGE_USER
USER $IMAGE_USER

# Ensure that /etc/profile does not clobber the existing path
RUN sed -i -E 's/((^|\s)PATH=)([^\$]*)$/\1\${PATH:-\3}/g' /etc/profile || true
"#.to_string();
        for feature in &self.features {
            let container_env_layer = feature.generate_dockerfile_env();
            dockerfile = format!("{dockerfile}\n{container_env_layer}");
        }

        if let Some(env) = &self.dev_container().container_env {
            for (key, value) in env {
                dockerfile = format!("{dockerfile}ENV {key}={value}\n");
            }
        }
        dockerfile
    }

    async fn build_feature_content_image(&self) -> Result<(), DevContainerError> {
        let Some(features_build_info) = &self.features_build_info else {
            log::error!("Features build info not available for building feature content image");
            return Err(DevContainerError::DevContainerParseFailed);
        };
        let features_content_dir = &features_build_info.features_content_dir;

        let dockerfile_content = "FROM scratch\nCOPY . /tmp/build-features/\n";
        let dockerfile_path = features_content_dir.join("Dockerfile.feature-content");

        self.fs
            .write(&dockerfile_path, dockerfile_content.as_bytes())
            .await
            .map_err(|e| {
                log::error!("Failed to write feature content Dockerfile: {e}");
                DevContainerError::FilesystemError
            })?;

        let mut command = Command::new(self.docker_client.docker_cli());
        // This path runs only when BuildKit is unavailable, so force the classic
        // builder: the feature content image is consumed by a later multi-stage
        // `FROM`, which requires it to live in the daemon's image store.
        if self.docker_client.docker_cli() != "podman" {
            command.env("DOCKER_BUILDKIT", "0");
        }
        command.args([
            "build",
            "-t",
            "dev_container_feature_content_temp",
            "-f",
            &dockerfile_path.display().to_string(),
            &features_content_dir.display().to_string(),
        ]);

        let output = self
            .command_runner
            .run_command(&mut command)
            .await
            .map_err(|e| {
                log::error!("Error building feature content image: {e}");
                DevContainerError::CommandFailed(self.docker_client.docker_cli())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("Feature content image build failed: {stderr}");
            return Err(DevContainerError::CommandFailed(
                self.docker_client.docker_cli(),
            ));
        }

        Ok(())
    }

    fn create_docker_build(&self) -> Result<Command, DevContainerError> {
        let dev_container = match &self.config {
            ConfigStatus::Deserialized(_) => {
                log::error!(
                    "Dev container has not yet been parsed for variable expansion. Cannot yet proceed with docker build"
                );
                return Err(DevContainerError::DevContainerParseFailed);
            }
            ConfigStatus::VariableParsed(dev_container) => dev_container,
        };

        let Some(features_build_info) = &self.features_build_info else {
            log::error!(
                "Cannot create docker build command; features build info has not been constructed"
            );
            return Err(DevContainerError::DevContainerParseFailed);
        };
        let mut command = Command::new(self.docker_client.docker_cli());

        command.args(["buildx", "build"]);

        // --load is short for --output=docker, loading the built image into the local docker images
        command.arg("--load");

        // BuildKit build context: provides the features content directory as a named context
        // that the Dockerfile.extended can COPY from via `--from=dev_containers_feature_content_source`
        command.args([
            "--build-context",
            &format!(
                "dev_containers_feature_content_source={}",
                features_build_info.features_content_dir.display()
            ),
        ]);

        // Build args matching the CLI reference implementation's `getFeaturesBuildOptions`
        if let Some(build_image) = &features_build_info.build_image {
            command.args([
                "--build-arg",
                &format!("_DEV_CONTAINERS_BASE_IMAGE={}", build_image),
            ]);
        } else {
            command.args([
                "--build-arg",
                "_DEV_CONTAINERS_BASE_IMAGE=dev_container_auto_added_stage_label",
            ]);
        }

        command.args([
            "--build-arg",
            &format!(
                "_DEV_CONTAINERS_IMAGE_USER={}",
                self.root_image
                    .as_ref()
                    .and_then(|docker_image| docker_image.config.image_user.as_ref())
                    .unwrap_or(&"root".to_string())
            ),
        ]);

        command.args([
            "--build-arg",
            "_DEV_CONTAINERS_FEATURE_CONTENT_SOURCE=dev_container_feature_content_temp",
        ]);

        if let Some(args) = dev_container.build.as_ref().and_then(|b| b.args.as_ref()) {
            for (key, value) in args {
                command.args(["--build-arg", &format!("{}={}", key, value)]);
            }
        }

        if let Some(options) = dev_container
            .build
            .as_ref()
            .and_then(|b| b.options.as_ref())
        {
            for option in options {
                command.arg(option);
            }
        }

        if let Some(cache_from_images) = dev_container
            .build
            .as_ref()
            .and_then(|b| b.cache_from.as_ref())
        {
            for cache_from_image in cache_from_images {
                command.args(["--cache-from", cache_from_image]);
            }
        }

        command.args(["--target", "dev_containers_target_stage"]);

        command.args([
            "-f",
            &features_build_info.dockerfile_path.display().to_string(),
        ]);

        command.args(["-t", &features_build_info.image_tag]);

        if let DevContainerBuildType::Dockerfile(build) = dev_container.build_type() {
            command.arg(self.calculate_context_dir(build).display().to_string());
        } else {
            // Use an empty folder as the build context to avoid pulling in unneeded files.
            // The actual feature content is supplied via the BuildKit build context above.
            command.arg(features_build_info.empty_context_dir.display().to_string());
        }

        Ok(command)
    }

    async fn run_docker_compose(
        &self,
        resources: DockerComposeResources,
    ) -> Result<DockerInspect, DevContainerError> {
        self.start_docker_compose_services(&resources, ComposeUpBehavior::Create)
            .await?;

        if let Some(docker_ps) = self.check_for_existing_container().await? {
            log::debug!("Found newly created dev container");
            return self.docker_client.inspect(&docker_ps.id).await;
        }

        log::error!("Could not find existing container after docker compose up");

        Err(DevContainerError::DevContainerParseFailed)
    }

    async fn start_docker_compose_services(
        &self,
        resources: &DockerComposeResources,
        behavior: ComposeUpBehavior,
    ) -> Result<(), DevContainerError> {
        let mut command = Command::new(self.docker_client.docker_cli());
        let project_name = self.project_name().await?;
        let compose_services = match self.dev_container().run_services.as_ref() {
            Some(run_services) => {
                let (main_service_name, _) = find_primary_service(&resources, self)?;
                compose_service_list(&main_service_name, Some(run_services))
            }
            None => None,
        };
        command.args(&["compose", "--project-name", &project_name]);
        for docker_compose_file in &resources.files {
            command.args(&["-f", &docker_compose_file.display().to_string()]);
        }
        command.args(&["up", "-d"]);
        if matches!(behavior, ComposeUpBehavior::Resume) {
            command.arg("--no-recreate");
        }
        if let Some(services) = compose_services.as_ref() {
            command.args(services);
        }

        let output = self
            .command_runner
            .run_command(&mut command)
            .await
            .map_err(|e| {
                log::error!("Error running docker compose up: {e}");
                DevContainerError::CommandFailed(command.get_program().display().to_string())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("Non-success status from docker compose up: {}", stderr);
            return Err(DevContainerError::CommandFailed(
                command.get_program().display().to_string(),
            ));
        }

        Ok(())
    }

    async fn run_docker_image(
        &self,
        build_resources: DockerBuildResources,
    ) -> Result<DockerInspect, DevContainerError> {
        let mut docker_run_command = self.create_docker_run_command(build_resources)?;

        let output = self
            .command_runner
            .run_command(&mut docker_run_command)
            .await
            .map_err(|e| {
                log::error!("Error running docker run: {e}");
                DevContainerError::CommandFailed(
                    docker_run_command.get_program().display().to_string(),
                )
            })?;

        if !output.status.success() {
            let std_err = String::from_utf8_lossy(&output.stderr);
            log::error!("Non-success status from docker run. StdErr: {std_err}");
            return Err(DevContainerError::CommandFailed(
                docker_run_command.get_program().display().to_string(),
            ));
        }

        log::debug!("Checking for container that was started");
        let Some(docker_ps) = self.check_for_existing_container().await? else {
            log::error!("Could not locate container just created");
            return Err(DevContainerError::DevContainerParseFailed);
        };
        self.docker_client.inspect(&docker_ps.id).await
    }

    fn local_workspace_folder(&self) -> String {
        self.local_project_directory.display().to_string()
    }
    fn local_workspace_base_name(&self) -> Result<String, DevContainerError> {
        self.local_project_directory
            .file_name()
            .map(|f| f.display().to_string())
            .ok_or(DevContainerError::DevContainerParseFailed)
    }

    fn remote_workspace_folder(&self) -> Result<PathBuf, DevContainerError> {
        self.dev_container()
            .workspace_folder
            .as_ref()
            .map(|folder| PathBuf::from(folder))
            .or(Some(
                // We explicitly use "/" here, instead of PathBuf::join
                // because we want remote targets to use unix-style filepaths,
                // even on a Windows host
                PathBuf::from(format!(
                    "{}/{}",
                    DEFAULT_REMOTE_PROJECT_DIR,
                    self.local_workspace_base_name()?
                )),
            ))
            .ok_or(DevContainerError::DevContainerParseFailed)
    }
    fn remote_workspace_base_name(&self) -> Result<String, DevContainerError> {
        self.remote_workspace_folder().and_then(|f| {
            f.file_name()
                .map(|file_name| file_name.display().to_string())
                .ok_or(DevContainerError::DevContainerParseFailed)
        })
    }

    fn remote_workspace_mount(&self) -> Result<MountDefinition, DevContainerError> {
        if let Some(mount) = &self.dev_container().workspace_mount {
            return Ok(mount.clone());
        }
        let Some(project_directory_name) = self.local_project_directory.file_name() else {
            return Err(DevContainerError::DevContainerParseFailed);
        };

        Ok(MountDefinition {
            source: Some(self.local_workspace_folder()),
            // We explicitly use "/" here, instead of PathBuf::join
            // because we want the remote target to use unix-style filepaths,
            // even on a Windows host
            target: format!(
                "{}/{}",
                PathBuf::from(DEFAULT_REMOTE_PROJECT_DIR).display(),
                project_directory_name.display()
            ),
            mount_type: None,
        })
    }

    fn create_docker_run_command(
        &self,
        build_resources: DockerBuildResources,
    ) -> Result<Command, DevContainerError> {
        let remote_workspace_mount = self.remote_workspace_mount()?;

        let docker_cli = self.docker_client.docker_cli();
        let mut command = Command::new(&docker_cli);

        command.arg("run");

        if build_resources.privileged {
            command.arg("--privileged");
        }
        if build_resources.init {
            command.arg("--init");
        }

        let run_args = match &self.dev_container().run_args {
            Some(run_args) => run_args,
            None => &Vec::new(),
        };

        for arg in run_args {
            command.arg(arg);
        }

        let run_if_missing = {
            |arg_name: &str, arg: &str, command: &mut Command| {
                if !run_args
                    .iter()
                    .any(|arg| arg.strip_prefix(arg_name).is_some())
                {
                    command.arg(arg);
                }
            }
        };

        if &docker_cli == "podman" {
            run_if_missing(
                "--security-opt",
                "--security-opt=label=disable",
                &mut command,
            );
            run_if_missing("--userns", "--userns=keep-id", &mut command);
        }

        run_if_missing("--sig-proxy", "--sig-proxy=false", &mut command);
        command.arg("-d");
        command.arg("--mount");
        command.arg(remote_workspace_mount.to_string());

        for mount in &build_resources.additional_mounts {
            command.arg("--mount");
            command.arg(mount.to_string());
        }

        for (key, val) in self.identifying_labels() {
            command.arg("-l");
            command.arg(format!("{}={}", key, val));
        }

        {
            let mut metadata_entries: Vec<serde_json_lenient::Value> = Vec::new();

            if let Some(image_metadata) = &build_resources.image.config.labels.metadata {
                for entry in image_metadata {
                    if let Ok(val) = serde_json_lenient::to_value(entry) {
                        metadata_entries.push(val);
                    }
                }
            }

            for feature in &self.features {
                let entry = feature.build_metadata_entry();
                if !entry.is_empty() {
                    metadata_entries.push(serde_json_lenient::Value::Object(entry));
                }
            }

            if let Ok(substituted_json) = self.parse_nonremote_vars_for_content(&self.raw_config) {
                let config_entry = build_devcontainer_metadata_entry(&substituted_json);
                if !config_entry.is_empty() {
                    metadata_entries.push(serde_json_lenient::Value::Object(config_entry));
                }
            }

            if !metadata_entries.is_empty() {
                let serialized = serde_json_lenient::to_string(&metadata_entries).map_err(|e| {
                    log::error!("Problem serializing metadata: {e}");
                    DevContainerError::ContainerNotValid(build_resources.image.id.clone())
                })?;
                command.arg("-l");
                command.arg(format!("devcontainer.metadata={serialized}"));
            }
        }

        for cap in &build_resources.cap_add {
            command.arg("--cap-add");
            command.arg(cap);
        }
        for opt in &build_resources.security_opt {
            command.arg("--security-opt");
            command.arg(opt);
        }

        if let Some(forward_ports) = &self.dev_container().forward_ports {
            for port in forward_ports {
                if let ForwardPort::Number(port_number) = port {
                    command.arg("-p");
                    command.arg(format!("{port_number}:{port_number}"));
                }
            }
        }
        for app_port in &self.dev_container().app_port {
            command.arg("-p");
            command.arg(app_port);
        }

        for (key, value) in &build_resources.container_env {
            command.arg("-e");
            command.arg(format!("{key}={value}"));
        }

        if let Some(entrypoint_script) = build_resources.entrypoint_script {
            command.arg("--entrypoint");
            command.arg("/bin/sh");
            command.arg(&build_resources.image_tag);
            command.arg("-c");
            command.arg(entrypoint_script);
            command.arg("-");
        } else {
            command.arg(&build_resources.image_tag);
        }

        Ok(command)
    }

    fn extension_ids(&self) -> Vec<String> {
        self.dev_container()
            .customizations
            .as_ref()
            .map(|c| c.zed.extensions.clone())
            .unwrap_or_default()
    }

    async fn build_and_run(&mut self) -> Result<DevContainerUp, DevContainerError> {
        self.dev_container().validate_devcontainer_contents()?;

        self.run_initialize_commands().await?;

        self.download_feature_and_dockerfile_resources().await?;

        let build_resources = self.build_resources().await?;

        let devcontainer_up = self.run_dev_container(build_resources).await?;

        self.run_remote_scripts(&devcontainer_up, true, true)
            .await?;

        Ok(devcontainer_up)
    }

    async fn run_remote_scripts(
        &self,
        devcontainer_up: &DevContainerUp,
        new_container: bool,
        container_started: bool,
    ) -> Result<(), DevContainerError> {
        let ConfigStatus::VariableParsed(config) = &self.config else {
            log::error!("Config not yet parsed, cannot proceed with remote scripts");
            return Err(DevContainerError::DevContainerScriptsFailed);
        };
        let remote_folder = self.remote_workspace_folder()?.display().to_string();

        if new_container {
            if let Some(on_create_command) = &config.on_create_command {
                for (command_name, command) in on_create_command.script_commands() {
                    log::debug!("Running on create command {command_name}");
                    self.docker_client
                        .run_docker_exec(
                            &devcontainer_up.container_id,
                            &remote_folder,
                            &devcontainer_up.remote_user,
                            &devcontainer_up.remote_env,
                            command,
                        )
                        .await?;
                }
            }
            if let Some(update_content_command) = &config.update_content_command {
                for (command_name, command) in update_content_command.script_commands() {
                    log::debug!("Running update content command {command_name}");
                    self.docker_client
                        .run_docker_exec(
                            &devcontainer_up.container_id,
                            &remote_folder,
                            &devcontainer_up.remote_user,
                            &devcontainer_up.remote_env,
                            command,
                        )
                        .await?;
                }
            }

            if let Some(post_create_command) = &config.post_create_command {
                for (command_name, command) in post_create_command.script_commands() {
                    log::debug!("Running post create command {command_name}");
                    self.docker_client
                        .run_docker_exec(
                            &devcontainer_up.container_id,
                            &remote_folder,
                            &devcontainer_up.remote_user,
                            &devcontainer_up.remote_env,
                            command,
                        )
                        .await?;
                }
            }
        }
        if let Some(post_start_command) = &config.post_start_command {
            let script_commands = post_start_command.script_commands();
            if let Some(started_at) = &devcontainer_up.started_at {
                if !script_commands.is_empty() {
                    for command_name in script_commands.keys() {
                        log::debug!("Running post start command {command_name}");
                    }
                    let script = post_start_marker_script(started_at, script_commands);
                    self.docker_client
                        .run_docker_exec(
                            &devcontainer_up.container_id,
                            &remote_folder,
                            &devcontainer_up.remote_user,
                            &devcontainer_up.remote_env,
                            Command::new(script),
                        )
                        .await?;
                }
            } else if container_started {
                for (command_name, command) in script_commands {
                    log::debug!("Running post start command {command_name}");
                    self.docker_client
                        .run_docker_exec(
                            &devcontainer_up.container_id,
                            &remote_folder,
                            &devcontainer_up.remote_user,
                            &devcontainer_up.remote_env,
                            command,
                        )
                        .await?;
                }
            }
        }
        if let Some(post_attach_command) = &config.post_attach_command {
            for (command_name, command) in post_attach_command.script_commands() {
                log::debug!("Running post attach command {command_name}");
                self.docker_client
                    .run_docker_exec(
                        &devcontainer_up.container_id,
                        &remote_folder,
                        &devcontainer_up.remote_user,
                        &devcontainer_up.remote_env,
                        command,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn run_initialize_commands(&self) -> Result<(), DevContainerError> {
        let ConfigStatus::VariableParsed(config) = &self.config else {
            log::error!("Config not yet parsed, cannot proceed with initializeCommand");
            return Err(DevContainerError::DevContainerParseFailed);
        };

        if let Some(initialize_command) = &config.initialize_command {
            log::debug!("Running initialize command");
            initialize_command
                .run(&self.command_runner, &self.local_project_directory)
                .await
        } else {
            log::warn!("No initialize command found");
            Ok(())
        }
    }

    async fn check_for_existing_devcontainer(
        &self,
    ) -> Result<Option<DevContainerUp>, DevContainerError> {
        if let Some(docker_ps) = self.check_for_existing_container().await? {
            log::debug!("Dev container already found. Proceeding with it");

            let mut docker_inspect = self.docker_client.inspect(&docker_ps.id).await?;

            let container_started = !docker_inspect.is_running();
            if container_started {
                log::debug!("Container not running. Will attempt to start, and then proceed");
                match self.dev_container().build_type() {
                    DevContainerBuildType::DockerCompose => {
                        let resources = self.docker_compose_manifest().await?;
                        self.start_docker_compose_services(&resources, ComposeUpBehavior::Resume)
                            .await?
                    }
                    _ => self.docker_client.start_container(&docker_ps.id).await?,
                }
                docker_inspect = self.docker_client.inspect(&docker_ps.id).await?;
            }

            let remote_user = get_remote_user_from_config(&docker_inspect, self)?;

            let remote_folder = self.remote_workspace_folder()?;

            let remote_env = self.runtime_remote_env(&docker_inspect.config.env_as_map()?)?;

            let dev_container_up = DevContainerUp {
                started_at: docker_inspect
                    .state
                    .as_ref()
                    .and_then(|state| state.started_at.clone()),
                container_id: docker_ps.id,
                remote_user: remote_user,
                remote_workspace_folder: remote_folder.display().to_string(),
                extension_ids: self.extension_ids(),
                remote_env,
            };

            self.run_remote_scripts(&dev_container_up, false, container_started)
                .await?;

            Ok(Some(dev_container_up))
        } else {
            log::debug!("Existing container not found.");

            Ok(None)
        }
    }

    async fn check_for_existing_container(&self) -> Result<Option<DockerPs>, DevContainerError> {
        self.docker_client
            .find_process_by_filters(
                self.identifying_labels()
                    .iter()
                    .map(|(k, v)| format!("label={k}={v}"))
                    .collect(),
            )
            .await
    }

    /// Matches `@devcontainers/cli`'s `getProjectName` in
    /// `src/spec-node/dockerCompose.ts`. See `derive_project_name` for the
    /// full precedence. Using the devcontainer.json `name` field here
    /// diverges from the reference CLI and creates duplicate compose
    /// projects when the same folder is opened by both tools — see #54255.
    ///
    /// Async because the derivation reads both the workspace `.env` file
    /// and the merged compose config — neither of which is available
    /// synchronously.
    async fn project_name(&self) -> Result<String, DevContainerError> {
        let workspace_fallback = self
            .local_workspace_base_name()
            .unwrap_or_else(|_| self.local_workspace_folder());
        let compose_resources = self.docker_compose_manifest().await.ok();
        let first_compose_file = compose_resources
            .as_ref()
            .and_then(|r| r.files.first())
            .map(PathBuf::as_path);
        let compose_config_name = compose_resources
            .as_ref()
            .and_then(|r| r.config.name.as_deref());
        let mut compose_name_explicitly_declared = false;
        if let Some(resources) = &compose_resources {
            for file in &resources.files {
                // Mirrors the CLI's fragment re-parse (dockerCompose.ts 663-673):
                // the whole readFile+yaml.load pair is wrapped in a single
                // try/catch that swallows every failure. The comment there
                // calls out `!reset` custom tags; the behavior is "on any
                // failure, treat the fragment as not-declared and keep
                // scanning." Propagating an I/O error here would diverge
                // from that policy and fail the whole devcontainer flow for
                // a fragment the CLI would have silently skipped.
                let contents = match self.fs.load(file).await {
                    Ok(contents) => contents,
                    Err(err) => {
                        log::warn!(
                            "Ignoring unreadable compose fragment `{}` while deriving project name: {err:?}",
                            file.display()
                        );
                        continue;
                    }
                };
                if compose_fragment_declares_name(&contents) {
                    compose_name_explicitly_declared = true;
                    break;
                }
            }
        }
        let dotenv_path = self.local_project_directory.join(".env");
        let dotenv_contents = match self.fs.load(&dotenv_path).await {
            Ok(contents) => Some(contents),
            Err(err) if is_missing_file_error(&err) => None,
            Err(err) => {
                // Mirrors the CLI: `getProjectName` only swallows `ENOENT`/
                // `EISDIR` on the `.env` read. Any other error (permission
                // denied, I/O failure, …) must surface so we don't silently
                // fall back to a non-canonical project name and create a
                // second compose project for the same repo.
                log::error!(
                    "Failed to read workspace .env `{}` while deriving project name: {err:?}",
                    dotenv_path.display()
                );
                return Err(DevContainerError::FilesystemError);
            }
        };
        Ok(derive_project_name(
            &self.local_environment,
            dotenv_contents.as_deref(),
            compose_config_name,
            compose_name_explicitly_declared,
            first_compose_file,
            &self.local_project_directory,
            &workspace_fallback,
        ))
    }

    async fn expanded_dockerfile_content(&self) -> Result<String, DevContainerError> {
        let Some(dockerfile_path) = self.dockerfile_location().await else {
            log::error!("Tried to expand dockerfile for an image-type config");
            return Err(DevContainerError::DevContainerParseFailed);
        };

        // For docker-compose configs the build args live on the primary
        // compose service rather than on dev_container.build.
        let devcontainer_args = match self.dev_container().build_type() {
            DevContainerBuildType::DockerCompose => {
                let compose = self.docker_compose_manifest().await?;
                find_primary_service(&compose, self)?
                    .1
                    .build
                    .and_then(|b| b.args)
                    .unwrap_or_default()
            }
            _ => self
                .dev_container()
                .build
                .as_ref()
                .and_then(|b| b.args.clone())
                .unwrap_or_default(),
        };
        let contents = self.fs.load(&dockerfile_path).await.map_err(|e| {
            log::error!("Failed to load Dockerfile: {e}");
            DevContainerError::FilesystemError
        })?;
        let mut parsed_lines: Vec<String> = Vec::new();
        let mut inline_args: Vec<(String, String)> = Vec::new();
        let key_regex = Regex::new(r"(?:^|\s)(\w+)=").expect("valid regex");

        for line in contents.lines() {
            let mut parsed_line = line.to_string();
            // Replace from devcontainer args first, since they take precedence
            for (key, value) in &devcontainer_args {
                parsed_line = expand_dockerfile_var(parsed_line, key, value);
            }
            for (key, value) in &inline_args {
                parsed_line = expand_dockerfile_var(parsed_line, key, value);
            }
            if let Some(arg_directives) = parsed_line.strip_prefix("ARG ") {
                let trimmed = arg_directives.trim();
                let key_matches: Vec<_> = key_regex.captures_iter(trimmed).collect();
                for (i, captures) in key_matches.iter().enumerate() {
                    let key = captures[1].to_string();
                    // Insert the devcontainer overrides here if needed
                    let value_start = captures.get(0).expect("full match").end();
                    let value_end = if i + 1 < key_matches.len() {
                        key_matches[i + 1].get(0).expect("full match").start()
                    } else {
                        trimmed.len()
                    };
                    let raw_value = trimmed[value_start..value_end].trim();
                    let value = if raw_value.starts_with('"')
                        && raw_value.ends_with('"')
                        && raw_value.len() > 1
                    {
                        &raw_value[1..raw_value.len() - 1]
                    } else {
                        raw_value
                    };
                    inline_args.push((key, value.to_string()));
                }
            }
            parsed_lines.push(parsed_line);
        }

        Ok(parsed_lines.join("\n"))
    }

    fn calculate_context_dir(&self, build: ContainerBuild) -> PathBuf {
        let Some(context) = build.context else {
            return self.config_directory.clone();
        };
        let context_path = PathBuf::from(context);

        if context_path.is_absolute() {
            context_path
        } else {
            self.config_directory.join(context_path)
        }
    }
}

/// Holds all the information needed to construct a `docker buildx build` command
/// that extends a base image with dev container features.
///
/// This mirrors the `ImageBuildOptions` interface in the CLI reference implementation
/// (cli/src/spec-node/containerFeatures.ts).
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct FeaturesBuildInfo {
    /// Path to the generated Dockerfile.extended
    pub dockerfile_path: PathBuf,
    /// Path to the features content directory (used as a BuildKit build context)
    pub features_content_dir: PathBuf,
    /// Path to an empty directory used as the Docker build context
    pub empty_context_dir: PathBuf,
    /// The base image name (e.g. "mcr.microsoft.com/devcontainers/rust:2-1-bookworm")
    pub build_image: Option<String>,
    /// The tag to apply to the built image (e.g. "vsc-myproject-features")
    pub image_tag: String,
}

pub(crate) async fn read_devcontainer_configuration(
    config: DevContainerConfig,
    context: &DevContainerContext,
    environment: HashMap<String, String>,
) -> Result<DevContainer, DevContainerError> {
    let docker = if context.use_podman {
        Docker::new("podman", context.use_buildkit).await
    } else {
        Docker::new("docker", context.use_buildkit).await
    };
    let mut dev_container = DevContainerManifest::new(
        context,
        environment,
        Arc::new(docker),
        Arc::new(DefaultCommandRunner::new()),
        config,
        &context.project_directory.as_ref(),
    )
    .await?;
    dev_container.parse_nonremote_vars()?;
    Ok(dev_container.dev_container().clone())
}

pub(crate) async fn spawn_dev_container(
    context: &DevContainerContext,
    environment: HashMap<String, String>,
    config: DevContainerConfig,
    local_project_path: &Path,
) -> Result<DevContainerUp, DevContainerError> {
    let docker = if context.use_podman {
        Docker::new("podman", context.use_buildkit).await
    } else {
        Docker::new("docker", context.use_buildkit).await
    };
    let mut devcontainer_manifest = DevContainerManifest::new(
        context,
        environment,
        Arc::new(docker),
        Arc::new(DefaultCommandRunner::new()),
        config,
        local_project_path,
    )
    .await?;

    devcontainer_manifest.parse_nonremote_vars()?;
    devcontainer_manifest
        .dev_container()
        .validate_environment_names()?;

    log::debug!("Checking for existing container");
    if let Some(devcontainer) = devcontainer_manifest
        .check_for_existing_devcontainer()
        .await?
    {
        Ok(devcontainer)
    } else {
        log::debug!("Existing container not found. Building");

        devcontainer_manifest.build_and_run().await
    }
}

#[derive(Debug)]
struct DockerBuildResources {
    image: DockerInspect,
    image_tag: String,
    additional_mounts: Vec<MountDefinition>,
    container_env: HashMap<String, String>,
    privileged: bool,
    init: bool,
    cap_add: Vec<String>,
    security_opt: Vec<String>,
    entrypoint_script: Option<String>,
}

#[derive(Debug)]
enum DevContainerBuildResources {
    DockerCompose(DockerComposeResources),
    Docker(DockerBuildResources),
}

/// Replaces occurrences of `${KEY}` and `$KEY` in `line` with `value`.
/// Bare `$KEY` is only replaced when the character immediately after the key
/// is not a word character (`[A-Za-z0-9_]`), so `$RUBY_VERSION2` is not
/// partially consumed when expanding `$RUBY_VERSION`.
fn expand_dockerfile_var(mut line: String, key: &str, value: &str) -> String {
    line = line.replace(&format!("${{{key}}}"), value);
    let pattern = format!("${key}");
    let mut result = String::with_capacity(line.len());
    let mut remaining = line.as_str();
    while let Some(pos) = remaining.find(pattern.as_str()) {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + pattern.len()..];
        if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            result.push('$');
            remaining = &remaining[pos + 1..];
        } else {
            result.push_str(value);
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

fn find_primary_service(
    docker_compose: &DockerComposeResources,
    devcontainer: &DevContainerManifest,
) -> Result<(String, DockerComposeService), DevContainerError> {
    let Some(service_name) = &devcontainer.dev_container().service else {
        return Err(DevContainerError::DevContainerParseFailed);
    };

    match docker_compose.config.services.get(service_name) {
        Some(service) => Ok((service_name.clone(), service.clone())),
        None => Err(DevContainerError::DevContainerParseFailed),
    }
}

/// The compose service list for `build` and `up`. Per the devcontainer spec
/// `runServices` are *additional*, so the primary `service` is always prepended.
/// `None` (no `runServices`) lets compose operate every service.
fn compose_service_list(
    main_service_name: &str,
    run_services: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    let run_services = run_services?;
    let mut services = Vec::with_capacity(run_services.len() + 1);
    services.push(main_service_name.to_string());
    for service in run_services {
        if service != main_service_name {
            services.push(service.clone());
        }
    }
    Some(services)
}

/// Resolves a compose service's dockerfile path according to the Docker Compose spec:
/// `dockerfile` is relative to the build `context`, and `context` is relative to
/// the compose file's directory.
fn resolve_compose_dockerfile(
    compose_file: &Path,
    context: Option<&str>,
    dockerfile: &str,
) -> Option<PathBuf> {
    let dockerfile = PathBuf::from(dockerfile);
    if dockerfile.is_absolute() {
        return Some(dockerfile);
    }
    let compose_dir = compose_file.parent()?;
    let context_dir = match context {
        Some(ctx) => {
            let ctx = PathBuf::from(ctx);
            if ctx.is_absolute() {
                ctx
            } else {
                normalize_path(&compose_dir.join(ctx))
            }
        }
        None => compose_dir.to_path_buf(),
    };
    Some(context_dir.join(dockerfile))
}

/// Destination folder inside the container where feature content is staged during build.
/// Mirrors the CLI's `FEATURES_CONTAINER_TEMP_DEST_FOLDER`.
const FEATURES_CONTAINER_TEMP_DEST_FOLDER: &str = "/tmp/dev-container-features";

/// Escapes regex special characters in a string.
fn escape_regex_chars(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    for c in input.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

/// Sanitize a string for use as a Docker Compose project name, matching
/// `@devcontainers/cli`'s `toProjectName` (modern Compose branch): lowercase
/// the input and strip any character outside `[-_a-z0-9]`.
fn sanitize_compose_project_name(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| c.to_lowercase())
        .filter(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || *c == '-' || *c == '_')
        .collect()
}

/// Derive the Docker Compose project name, mirroring `getProjectName` in
/// `@devcontainers/cli`'s `src/spec-node/dockerCompose.ts`. Precedence:
///
/// 1. `COMPOSE_PROJECT_NAME` from the local environment.
/// 2. `COMPOSE_PROJECT_NAME` from the workspace `.env` file.
/// 3. The top-level `name:` field of the merged compose config, but only
///    when at least one compose fragment explicitly declared `name:`.
///    Compose injects a default `name: devcontainer` into its merged
///    output whenever no fragment declared one — that default must NOT be
///    treated as a user-provided name, so rule 4 applies instead.
/// 4. Basename of the first compose file's directory, appending
///    `_devcontainer` only when that directory is
///    `<workspace_root>/.devcontainer`.
///
/// The caller is responsible for computing `compose_name_explicitly_declared`
/// by scanning the original compose fragments for a top-level `name:` key
/// (the reference CLI does the same). This keeps the helper a pure function
/// of its inputs.
///
/// All branches pass through `sanitize_compose_project_name` — the CLI's
/// final normalization step.
fn derive_project_name(
    local_environment: &HashMap<String, String>,
    workspace_dotenv_contents: Option<&str>,
    compose_config_name: Option<&str>,
    compose_name_explicitly_declared: bool,
    first_compose_file: Option<&Path>,
    workspace_root: &Path,
    workspace_fallback: &str,
) -> String {
    if let Some(env_name) = local_environment.get("COMPOSE_PROJECT_NAME")
        && !env_name.is_empty()
    {
        return sanitize_compose_project_name(env_name);
    }
    if let Some(contents) = workspace_dotenv_contents
        && let Some(dotenv_name) = parse_dotenv_compose_project_name(contents)
        && !dotenv_name.is_empty()
    {
        return sanitize_compose_project_name(&dotenv_name);
    }
    if let Some(name) = compose_config_name
        && !name.is_empty()
        && compose_name_explicitly_declared
    {
        return sanitize_compose_project_name(name);
    }
    let compose_dir = first_compose_file.and_then(Path::parent);
    let canonical_devcontainer_dir = normalize_path(&workspace_root.join(".devcontainer"));
    let raw = match compose_dir {
        Some(dir) if dir == canonical_devcontainer_dir => {
            // Matches the CLI's `configDir/.devcontainer` branch: use the
            // *workspace root's* basename with the `_devcontainer` suffix,
            // NOT the `.devcontainer` dir's basename.
            format!("{workspace_fallback}_devcontainer")
        }
        Some(dir) => dir
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| workspace_fallback.to_string()),
        None => format!("{workspace_fallback}_devcontainer"),
    };
    sanitize_compose_project_name(&raw)
}

/// Classify an anyhow error from `Fs::load` as "file does not exist" vs a
/// real I/O failure. Used on the `.env` read in `project_name()`, where the
/// CLI's `getProjectName` catches only `ENOENT`/`EISDIR` and rethrows
/// everything else; any other error must propagate so callers can surface
/// the problem instead of silently falling back to a non-canonical project
/// name. (The fragment-rescan loop uses a different, broader swallow —
/// the CLI wraps its fragment read+parse in one try/catch that ignores
/// every failure.)
fn is_missing_file_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>().is_some_and(|e| {
        matches!(
            e.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::IsADirectory
        )
    })
}

/// Extract `COMPOSE_PROJECT_NAME` from a `.env` file's contents. Matches
/// the subset of dotenv syntax that `@devcontainers/cli`'s regex parser
/// recognizes: a bare `COMPOSE_PROJECT_NAME=value` line (no `export` prefix,
/// no quoting, no line continuation). Comment lines are skipped.
fn parse_dotenv_compose_project_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("COMPOSE_PROJECT_NAME=") {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Detect whether a compose-file fragment declares a top-level `name:` key.
/// Matches the reference CLI's approach: parse the fragment as YAML and check
/// for a `name` key on the root mapping. This handles all valid styles —
/// block mappings, quoted keys (`"name":`), flow-style root mappings, anchors,
/// etc. On parse failure we fall through (return `false`), matching the CLI's
/// own behavior when fragment parsing errors.
fn compose_fragment_declares_name(contents: &str) -> bool {
    let Ok(docs) = yaml_rust2::YamlLoader::load_from_str(contents) else {
        return false;
    };
    let Some(yaml_rust2::Yaml::Hash(h)) = docs.into_iter().next() else {
        return false;
    };
    h.contains_key(&yaml_rust2::Yaml::String("name".to_string()))
}

/// Extracts the short feature ID from a full feature reference string.
///
/// Examples:
/// - `ghcr.io/devcontainers/features/aws-cli:1` → `aws-cli`
/// - `ghcr.io/user/repo/go` → `go`
/// - `ghcr.io/devcontainers/features/rust@sha256:abc` → `rust`
/// - `./myFeature` → `myFeature`
fn extract_feature_id(feature_ref: &str) -> &str {
    let without_version = if let Some(at_idx) = feature_ref.rfind('@') {
        &feature_ref[..at_idx]
    } else {
        let last_slash = feature_ref.rfind('/');
        let last_colon = feature_ref.rfind(':');
        match (last_slash, last_colon) {
            (Some(slash), Some(colon)) if colon > slash => &feature_ref[..colon],
            _ => feature_ref,
        }
    };
    match without_version.rfind('/') {
        Some(idx) => &without_version[idx + 1..],
        None => without_version,
    }
}

fn is_local_feature_ref(feature_ref: &str) -> bool {
    feature_ref.starts_with("./") || feature_ref.starts_with("../")
}

/// Generates a shell command that looks up a user's passwd entry.
///
/// Mirrors the CLI's `getEntPasswdShellCommand` in `commonUtils.ts`.
/// Tries `getent passwd` first, then falls back to grepping `/etc/passwd`.
fn get_ent_passwd_shell_command(user: &str) -> String {
    let escaped_for_shell = user.replace('\\', "\\\\").replace('\'', "\\'");
    let escaped_for_regex = escape_regex_chars(user).replace('\'', "\\'");
    format!(
        " (command -v getent >/dev/null 2>&1 && getent passwd '{shell}' || grep -E '^{re}|^[^:]*:[^:]*:{re}:' /etc/passwd || true)",
        shell = escaped_for_shell,
        re = escaped_for_regex,
    )
}

/// Determines feature installation order, respecting `overrideFeatureInstallOrder`.
///
/// Features listed in the override come first (in the specified order), followed
/// by any remaining features sorted lexicographically by their full reference ID.
fn resolve_feature_order<'a>(
    features: &'a HashMap<String, FeatureOptions>,
    override_order: &Option<Vec<String>>,
) -> Vec<(&'a String, &'a FeatureOptions)> {
    if let Some(order) = override_order {
        let mut ordered: Vec<(&'a String, &'a FeatureOptions)> = Vec::new();
        for ordered_id in order {
            if let Some((key, options)) = features.get_key_value(ordered_id) {
                ordered.push((key, options));
            }
        }
        let mut remaining: Vec<_> = features
            .iter()
            .filter(|(id, _)| !order.iter().any(|o| o == *id))
            .collect();
        remaining.sort_by_key(|(id, _)| id.as_str());
        ordered.extend(remaining);
        ordered
    } else {
        let mut entries: Vec<_> = features.iter().collect();
        entries.sort_by_key(|(id, _)| id.as_str());
        entries
    }
}

/// Generates the `devcontainer-features-install.sh` wrapper script for one feature.
///
/// Mirrors the CLI's `getFeatureInstallWrapperScript` in
/// `containerFeaturesConfiguration.ts`.
fn generate_install_wrapper(
    feature_ref: &str,
    feature_id: &str,
    env_variables: &str,
) -> Result<String, DevContainerError> {
    let escaped_id = shlex::try_quote(feature_ref).map_err(|e| {
        log::error!("Error escaping feature ref {feature_ref}: {e}");
        DevContainerError::DevContainerParseFailed
    })?;
    let escaped_name = shlex::try_quote(feature_id).map_err(|e| {
        log::error!("Error escaping feature {feature_id}: {e}");
        DevContainerError::DevContainerParseFailed
    })?;
    let options_indented: String = env_variables
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| format!("    {}", l))
        .collect::<Vec<_>>()
        .join("\n");
    let escaped_options = shlex::try_quote(&options_indented).map_err(|e| {
        log::error!("Error escaping options {options_indented}: {e}");
        DevContainerError::DevContainerParseFailed
    })?;

    let script = format!(
        r#"#!/bin/sh
set -e

on_exit () {{
    [ $? -eq 0 ] && exit
    echo 'ERROR: Feature "{escaped_name}" ({escaped_id}) failed to install!'
}}

trap on_exit EXIT

echo ===========================================================================
echo 'Feature       : {escaped_name}'
echo 'Id            : {escaped_id}'
echo 'Options       :'
echo {escaped_options}
echo ===========================================================================

set -a
. ../devcontainer-features.builtin.env
. ./devcontainer-features.env
set +a

chmod +x ./install.sh
./install.sh
"#
    );

    Ok(script)
}

struct ParsedFromLine<'a> {
    image: &'a str,
    alias: Option<&'a str>,
}

/// Parses a `FROM` instruction into its image and optional stage alias,
/// skipping flags like `--platform=...`. Returns `None` for non-`FROM` lines.
fn parse_from_line(line: &str) -> Option<ParsedFromLine<'_>> {
    let mut tokens = line.split_whitespace();
    if !tokens.next()?.eq_ignore_ascii_case("FROM") {
        return None;
    }
    let image = tokens.find(|token| !token.starts_with("--"))?;
    let alias = match (tokens.next(), tokens.next()) {
        (Some(keyword), Some(alias)) if keyword.eq_ignore_ascii_case("as") => Some(alias),
        _ => None,
    };
    Some(ParsedFromLine { image, alias })
}

fn dockerfile_inject_alias(
    dockerfile_content: &str,
    alias: &str,
    build_target: Option<String>,
) -> String {
    let from_lines: Vec<(usize, ParsedFromLine)> = dockerfile_content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_from_line(line).map(|parsed| (index, parsed)))
        .collect();

    let target_entry = match &build_target {
        Some(target) => from_lines.iter().rfind(|(_, parsed)| {
            parsed
                .alias
                .is_some_and(|alias| alias.eq_ignore_ascii_case(target))
        }),
        None => from_lines.last(),
    };

    let Some((line_idx, parsed)) = target_entry else {
        match &build_target {
            Some(target) => log::warn!(
                "Build target stage {target:?} not found in Dockerfile; leaving it unmodified"
            ),
            None => log::warn!("No FROM instruction found in Dockerfile; leaving it unmodified"),
        }
        return dockerfile_content.to_string();
    };

    if let Some(existing_alias) = parsed.alias {
        format!("{dockerfile_content}\nFROM {existing_alias} AS {alias}")
    } else {
        let lines: Vec<&str> = dockerfile_content.lines().collect();
        // Appending ` AS {alias}` to a line ending in a `\` continuation would
        // corrupt the instruction, so leave the Dockerfile unmodified.
        if lines
            .get(*line_idx)
            .is_some_and(|line| line.trim_end().ends_with('\\'))
        {
            log::warn!(
                "FROM instruction spans multiple lines via `\\` continuation; cannot inject stage alias, leaving Dockerfile unmodified"
            );
            return dockerfile_content.to_string();
        }
        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                result.push('\n');
            }
            if i == *line_idx {
                result.push_str(&format!("{line} AS {alias}"));
            } else {
                result.push_str(line);
            }
        }
        if dockerfile_content.ends_with('\n') {
            result.push('\n');
        }
        result
    }
}

fn image_from_dockerfile(dockerfile_contents: String, target: &Option<String>) -> Option<String> {
    let stages: Vec<ParsedFromLine> = dockerfile_contents
        .lines()
        .filter_map(parse_from_line)
        .collect();

    let start_index = match target {
        Some(target) => stages.iter().rposition(|stage| {
            stage
                .alias
                .is_some_and(|alias| alias.eq_ignore_ascii_case(target))
        })?,
        None => stages.len().checked_sub(1)?,
    };

    // Follow alias chains (`FROM base AS development`) to a concrete image.
    // Docker only resolves names to stages defined earlier in the file, so
    // resolving strictly backwards is correct and cannot cycle.
    let mut index = start_index;
    loop {
        let image = stages.get(index)?.image;
        let previous_stage = stages.get(..index)?.iter().rposition(|stage| {
            stage
                .alias
                .is_some_and(|alias| alias.eq_ignore_ascii_case(image))
        });
        match previous_stage {
            Some(previous_index) => index = previous_index,
            None => return Some(image.to_string()),
        }
    }
}

fn get_remote_user_from_config(
    docker_config: &DockerInspect,
    devcontainer: &DevContainerManifest,
) -> Result<String, DevContainerError> {
    if let DevContainer {
        remote_user: Some(user),
        ..
    } = &devcontainer.dev_container()
    {
        return Ok(user.clone());
    }
    if let Some(metadata) = &docker_config.config.labels.metadata {
        for metadatum in metadata {
            if let Some(remote_user) = metadatum.get("remoteUser") {
                if let Some(remote_user_str) = remote_user.as_str() {
                    return Ok(remote_user_str.to_string());
                }
            }
        }
    }
    if let Some(image_user) = &docker_config.config.image_user {
        if !image_user.is_empty() {
            return Ok(image_user.to_string());
        }
    }
    Ok("root".to_string())
}

// This should come from spec - see the docs
fn get_container_user_from_config(
    docker_config: &DockerInspect,
    devcontainer: &DevContainerManifest,
) -> Result<String, DevContainerError> {
    if let Some(user) = &devcontainer.dev_container().container_user {
        return Ok(user.to_string());
    }
    if let Some(metadata) = &docker_config.config.labels.metadata {
        for metadatum in metadata {
            if let Some(container_user) = metadatum.get("containerUser") {
                if let Some(container_user_str) = container_user.as_str() {
                    return Ok(container_user_str.to_string());
                }
            }
        }
    }
    if let Some(image_user) = &docker_config.config.image_user {
        return Ok(image_user.to_string());
    }

    Ok("root".to_string())
}

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

fn append_mount_with_target_override(mounts: &mut Vec<MountDefinition>, mount: MountDefinition) {
    mounts.retain(|existing| existing.target != mount.target);
    mounts.push(mount);
}

fn mount_definition_from_metadata(value: &serde_json_lenient::Value) -> Option<MountDefinition> {
    match value {
        serde_json_lenient::Value::String(value) => parse_mount_definition_string(value),
        serde_json_lenient::Value::Object(object) => {
            let source = object
                .get("source")
                .or_else(|| object.get("src"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let target = object
                .get("target")
                .or_else(|| object.get("dst"))
                .or_else(|| object.get("destination"))
                .and_then(|value| value.as_str())?
                .to_string();
            let mount_type = object
                .get("type")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            Some(MountDefinition {
                source,
                target,
                mount_type,
            })
        }
        _ => None,
    }
}

fn parse_mount_definition_string(value: &str) -> Option<MountDefinition> {
    let mut source = None;
    let mut target = None;
    let mut mount_type = None;

    for part in value.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        match key.trim() {
            "source" | "src" => source = Some(value.trim().to_string()),
            "target" | "dst" | "destination" => target = Some(value.trim().to_string()),
            "type" => mount_type = Some(value.trim().to_string()),
            _ => {}
        }
    }

    target.map(|target| MountDefinition {
        source,
        target,
        mount_type,
    })
}

fn escape_compose_interpolation(value: &str) -> String {
    value.replace('$', "$$")
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn command_to_shell_string(command: &Command) -> String {
    let mut command_parts = vec![command.get_program().display().to_string()];
    command_parts.extend(command.get_args().map(|arg| arg.display().to_string()));
    command_parts.join(" ")
}

fn post_start_marker_script(started_at: &str, script_commands: HashMap<String, Command>) -> String {
    let started_at = shell_single_quote(started_at);
    let mut script = format!(
        r#"user_id="$(id -u)"
home_directory="${{HOME:-}}"
if [ -z "$home_directory" ]; then
  home_directory="$( (command -v getent >/dev/null 2>&1 && getent passwd "$user_id" || grep -E "^[^:]*:[^:]*:${{user_id}}:" /etc/passwd || true) | head -n 1 | cut -d: -f6)"
fi
if [ -z "$home_directory" ]; then
  home_directory=/root
fi
marker_directory="$home_directory/.devcontainer"
marker="$marker_directory/.postStartCommandMarker"
started_at={started_at}
marker_available=false
if mkdir -p "$marker_directory"; then
  previous_started_at="$(cat "$marker" 2>/dev/null || true)"
  if [ "$previous_started_at" = "$started_at" ]; then
    exit 0
  fi
  marker_available=true
fi
"#,
    );
    for command in script_commands.into_values() {
        script.push_str(&command_to_shell_string(&command));
        script.push_str("\ncommand_status=$?\n");
        script.push_str("[ \"$command_status\" -eq 0 ] || exit \"$command_status\"\n");
    }
    script.push_str(
        r#"if [ "$marker_available" = "true" ]; then
  printf '%s' "$started_at" > "$marker" || exit $?
fi"#,
    );
    script
}

fn build_devcontainer_metadata_entry(
    raw_json: &serde_json_lenient::Value,
) -> serde_json_lenient::Map<String, serde_json_lenient::Value> {
    use serde_json_lenient::Value;
    static METADATA_PROPERTIES: &[&str] = &[
        "onCreateCommand",
        "updateContentCommand",
        "postCreateCommand",
        "postStartCommand",
        "postAttachCommand",
        "waitFor",
        "customizations",
        "mounts",
        "containerEnv",
        "containerUser",
        "init",
        "privileged",
        "capAdd",
        "securityOpt",
        "remoteUser",
        "userEnvProbe",
        "remoteEnv",
        "overrideCommand",
        "portsAttributes",
        "otherPortsAttributes",
        "forwardPorts",
        "shutdownAction",
        "updateRemoteUserUID",
        "hostRequirements",
    ];

    let Value::Object(full) = raw_json else {
        return serde_json_lenient::Map::new();
    };

    full.iter()
        .filter(|(key, value)| METADATA_PROPERTIES.contains(&key.as_str()) && !value.is_null())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn normalize_label_path(path: &str) -> String {
    path.to_string()
}
