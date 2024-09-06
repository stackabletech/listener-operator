# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

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
