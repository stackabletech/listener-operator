# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Generate OLM bundle for Release 23.4.0 ([#74]).
- Provide automatic migration 23.1 -> 23.4 ([#77]).

[#74]: https://github.com/stackabletech/listener-operator/pull/74
[#77]: https://github.com/stackabletech/listener-operator/pull/77

## [23.4.0] - 2023-04-17

### Added

- Allow configuring CSI docker images ([#61]).

### Changed

- Shortened the registration socket path for Microk8s compatibility ([#45]).
  - The old CSI registration path will be automatically migrated during upgrade to `23.4.1` ([#77]).
  - You might need to manually remove `/var/lib/kubelet/plugins_registry/listeners.stackable.tech-reg.sock` when downgrading.

[#61]: https://github.com/stackabletech/listener-operator/pull/61

## [23.1.0] - 2023-01-23

### Changed

- Helm installation on OpenShift ([#29]).
- `operator-rs` `0.25.2` -> `0.27.1` ([#34]).
- Made kubeletDir configurable ([#45]).
  - Microk8s users will need to `--set kubeletDir=/var/snap/microk8s/common/var/lib/kubelet`.

[#29]: https://github.com/stackabletech/listener-operator/pull/29
[#34]: https://github.com/stackabletech/listener-operator/pull/34
[#45]: https://github.com/stackabletech/listener-operator/pull/45
[#77]: https://github.com/stackabletech/listener-operator/pull/77
