# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

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
