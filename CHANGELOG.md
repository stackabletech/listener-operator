# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- New helm values for `*.priority`, `*.priorityClassName`, and `*.preemptionPolicy` ([#334]).

### Changed

- Split helm values for independent configuration ([#334]).
  - `controller` values have been moved to `csiProvisioner.controllerService`.
  - `csiProvisioner` values have been moved to `csiProvisioner.externalProvisioner`
  - `csiNodeDriverRegistrar` values have been moved to `csiNodeDriver.nodeRegistrar`.
  - `node.driver` values have been moved to `csiNodeDriver.nodeService`.
  - `podAnnotations` has been split into `csiProvisioner.podAnnotations` and `csiNodeDriver.podAnnotations`.
  - `podSecurityContext` has been split into `csiProvisioner.podSecurityContext` and `csiNodeDriver.podSecurityContext`.
  - `nodeSelector` has been split into `csiProvisioner.nodeSelector` and `csiNodeDriver.nodeSelector`.
  - `tolerations` has been split into `csiProvisioner.tolerations` and `csiNodeDriver.tolerations`.
  - `affinity` has been split into `csiProvisioner.affinity` and `csiNodeDriver.affinity`.

[#334]: https://github.com/stackabletech/listener-operator/pull/334

## [25.7.0] - 2025-07-23

## [25.7.0-rc1] - 2025-07-18

### Added

- Adds new telemetry CLI arguments and environment variables ([#299]).
  - Use `--file-log-max-files` (or `FILE_LOG_MAX_FILES`) to limit the number of log files kept.
  - Use `--file-log-rotation-period` (or `FILE_LOG_ROTATION_PERIOD`) to configure the frequency of rotation.
  - Use `--console-log-format` (or `CONSOLE_LOG_FORMAT`) to set the format to `plain` (default) or `json`.
- Added support for configuring `Service.spec.loadBalancerClass` and `.allocateLoadBalancerNodePorts` ([#288]).
- Add RBAC rule to helm template for automatic cluster domain detection ([#320]).

### Changed

- BREAKING: Replace stackable-operator `initialize_logging` with stackable-telemetry `Tracing` ([#291], [#299]).
  - operator-binary
    - The console log level was set by `LISTENER_OPERATOR_LOG`, and is now set by `CONSOLE_LOG_LEVEL`.
    - The file log level was set by `LISTENER_OPERATOR_LOG`, and is now set by `FILE_LOG_LEVEL`.
    - The file log directory was set by `LISTENER_OPERATOR_LOG_DIRECTORY`, and is now set
      by `FILE_LOG_DIRECTORY` (or via `--file-log-directory <DIRECTORY>`).
  - olm-deployer
    - The console log level was set by `STKBL_LISTENER_OLM_DEPLOYER_LOG`, and is now set by `CONSOLE_LOG_LEVEL`.
    - The file log level was set by `STKBL_LISTENER_OLM_DEPLOYER_LOG`, and is now set by `FILE_LOG_LEVEL`.
    - The file log directory was set by `STKBL_LISTENER_OLM_DEPLOYER_LOG_DIRECTORY`, and is now set
      by `FILE_LOG_DIRECTORY` (or via `--file-log-directory <DIRECTORY>`).
  - Replace stackable-operator `print_startup_string` with `tracing::info!` with fields.
- Upgrade csi-provisioner to 5.2.0 ([#304]).
- Version CRDs and bump dependencies ([#307]).
- BREAKING: Bump stackable-operator to 0.94.0 and update other dependencies ([#320]).
  - The default Kubernetes cluster domain name is now fetched from the kubelet API unless explicitly configured.
  - This requires operators to have the RBAC permission to get nodes/proxy in the apiGroup "". The helm-chart takes care of this.
  - The CLI argument `--kubernetes-node-name` or env variable `KUBERNETES_NODE_NAME` needs to be set.
    It supersedes the old argument/env variable `NODE_NAME`.
    The helm-chart takes care of this.

### Fixed

- Allow uppercase characters in domain names ([#320]).

### Removed

- Remove the `lastUpdateTime` field from the stacklet status ([#320]).
- Remove role binding to legacy service accounts ([#320]).

[#288]: https://github.com/stackabletech/listener-operator/pull/288
[#291]: https://github.com/stackabletech/listener-operator/pull/291
[#299]: https://github.com/stackabletech/listener-operator/pull/299
[#304]: https://github.com/stackabletech/listener-operator/pull/304
[#307]: https://github.com/stackabletech/listener-operator/pull/307
[#320]: https://github.com/stackabletech/listener-operator/pull/320

## [25.3.0] - 2025-03-21

### Added

- Aggregate emitted Kubernetes events on the CustomResources ([#267]).
- OLM deployment helper ([#279]).

### Changed

- Bump `stackable-operator` to 0.87.0 ([#282]).
- Default to OCI for image metadata ([#268]).

### Fixed

- Give RBAC permission to `delete` Services, which is needed to set an ownerRef on already existing Services ([#283]).
- Fix the error "failed to write content: File exists (os error 17)" after a
  Node restart ([#284]).

[#267]: https://github.com/stackabletech/listener-operator/pull/267
[#268]: https://github.com/stackabletech/listener-operator/pull/268
[#279]: https://github.com/stackabletech/listener-operator/pull/279
[#282]: https://github.com/stackabletech/listener-operator/pull/282
[#283]: https://github.com/stackabletech/listener-operator/pull/283
[#284]: https://github.com/stackabletech/listener-operator/pull/284

## [24.11.1] - 2025-01-10

## [24.11.0] - 2024-11-18

### Added

- `Listener.status.addresses` can now be configured to prefer either IP addresses or DNS hostnames ([#233], [#244]).
- The operator can now run on Kubernetes clusters using a non-default cluster domain.
  Use the env var `KUBERNETES_CLUSTER_DOMAIN` or the operator Helm chart property `kubernetesClusterDomain` to set a non-default cluster domain ([#237]).

### Changed

- `Listener.status.addresses` for NodePort listeners now includes replicas that are currently unavailable ([#231]).
- BREAKING: `Listener.status.addresses` now defaults to DNS hostnames for ClusterIP services, rather than IP addresses ([#233], [#244]).
- Stale Listener subobjects will now be deleted ([#232]).
- Tagged Listener Services with the SDP labels ([#232]).

### Fixed

- Listener.status.addresses is now de-duplicated ([#231]).
- Listener controller now listens for ListenerClass updates ([#231]).
- Propagate `ListenerClass.spec.serviceAnnotations` to the created Services ([#234]).
- Failing to parse one `Listener`/`ListenerClass` should no longer cause the whole operator to stop functioning ([#238]).
- Added necessary RBAC permissions for running on Openshift ([#246]).

[#231]: https://github.com/stackabletech/listener-operator/pull/231
[#232]: https://github.com/stackabletech/listener-operator/pull/232
[#233]: https://github.com/stackabletech/listener-operator/pull/233
[#234]: https://github.com/stackabletech/listener-operator/pull/234
[#237]: https://github.com/stackabletech/listener-operator/pull/237
[#238]: https://github.com/stackabletech/listener-operator/pull/238
[#244]: https://github.com/stackabletech/listener-operator/pull/244
[#246]: https://github.com/stackabletech/listener-operator/pull/246

## [24.7.0] - 2024-07-24

### Added

- Propagate `external_traffic_policy` from ListenerClass to created Services ([#196]).
- Chore: Upgrade csi-provisioner to 5.0.1 and csi-node-driver-registrar to 2.11.1 ([#203])

### Changed

- Update the image docker.stackable.tech/k8s/sig-storage/csi-provisioner
  in the Helm values to v4.0.1 ([#194]).
- Update the image docker.stackable.tech/k8s/sig-storage/csi-node-driver-registrar
  in the Helm values to v2.10.1 ([#194]).
- Remove custom `h2` patch, as Kubernetes 1.26 has fixed the invalid data from Kubernetes' side. Starting with 24.11 we only support at least 1.27 (as it's needed by OpenShift 4.14) ([#219]).

### Removed

- Init container deployed by the Helm chart as part of the daemonset. It was added a an automatic migration between SDP versions and is not needed anymore  ([#174]).

### Fixed

- Propagate labels from `Listener`s to the created `Service`s ([#169]).

[#169]: https://github.com/stackabletech/listener-operator/pull/169
[#174]: https://github.com/stackabletech/listener-operator/pull/174
[#194]: https://github.com/stackabletech/listener-operator/pull/194
[#196]: https://github.com/stackabletech/listener-operator/pull/196
[#203]: https://github.com/stackabletech/listener-operator/pull/203
[#219]: https://github.com/stackabletech/listener-operator/pull/219

## [24.3.0] - 2024-03-20

### Added

- Helm: support labels in values.yaml ([#142]).
- Propagate labels from PVCs to Listener objects ([#158]).

### Fixed

- Replace "Release.Name" with "operator.fullname" in Helm resource names ([#131])

[#131]: https://github.com/stackabletech/listener-operator/pull/131
[#142]: https://github.com/stackabletech/listener-operator/pull/142
[#158]: https://github.com/stackabletech/listener-operator/pull/158

## [23.11.0] - 2023-11-24

### Added

- Write `PodListeners` objects for mounted listener volumes ([#100]).

### Fixed

- Fixed pods being unable to bind listeners with long names ([#111]).

### Changed

- Remove the requirement for privileged mode ([#101]).
- Listener volume mounting is now enforced ([#105], [#111]).

[#100]: https://github.com/stackabletech/listener-operator/pull/100
[#101]: https://github.com/stackabletech/listener-operator/pull/101
[#105]: https://github.com/stackabletech/listener-operator/pull/105
[#111]: https://github.com/stackabletech/listener-operator/pull/111

## [23.7.0] - 2023-07-14

### Added

- Generate OLM bundle for Release 23.4.0 ([#74]).
- Provide automatic migration 23.1 -> 23.4 ([#77]).
- Support ClusterIP service type ([#83]).

[#83]: https://github.com/stackabletech/listener-operator/pull/83

### Changed

- `operator-rs` `0.27.1` -> `0.44.0` ([#80], [#83]).
- Defined resource limits for all Deployments and Daemonsets ([#85]).

[#74]: https://github.com/stackabletech/listener-operator/pull/74
[#80]: https://github.com/stackabletech/listener-operator/pull/80
[#85]: https://github.com/stackabletech/listener-operator/pull/85

## [23.4.0] - 2023-04-17

### Added

- Allow configuring CSI docker images ([#61]).

### Changed

- Shortened the registration socket path for Microk8s compatibility ([#45]).
  - The old CSI registration path will be automatically migrated during upgrade to `23.4.1` ([#77]).
  - You might need to manually remove `/var/lib/kubelet/plugins_registry/listeners.stackable.tech-reg.sock` when downgrading.

[#61]: https://github.com/stackabletech/listener-operator/pull/61
[#77]: https://github.com/stackabletech/listener-operator/pull/77

## [23.1.0] - 2023-01-23

### Changed

- Helm installation on OpenShift ([#29]).
- `operator-rs` `0.25.2` -> `0.27.1` ([#34]).
- Made kubeletDir configurable ([#45]).
  - Microk8s users will need to `--set kubeletDir=/var/snap/microk8s/common/var/lib/kubelet`.

[#29]: https://github.com/stackabletech/listener-operator/pull/29
[#34]: https://github.com/stackabletech/listener-operator/pull/34
[#45]: https://github.com/stackabletech/listener-operator/pull/45
