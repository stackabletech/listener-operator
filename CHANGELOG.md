# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Generate OLM bundle for Release 23.4.0 ([#74]).

[#74]: https://github.com/stackabletech/listener-operator/pull/74

## [23.4.0] - 2023-04-17

### Added

- Allow configuring CSI docker images ([#61]).

[#61]: https://github.com/stackabletech/listener-operator/pull/61

## [23.1.0] - 2023-01-23

### Changed

- Helm installation on OpenShift ([#29]).
- `operator-rs` `0.25.2` -> `0.27.1` ([#34]).
- Shortened the registration socket path for Microk8s compatibility ([#45]).
  - After upgrading you will need to
    `rmdir /var/lib/kubelet/plugins_registry/listeners.stackable.tech-reg.sock` manually.
    This applies to *all* users, not just Microk8s.
- Made kubeletDir configurable ([#45]).
  - Microk8s users will need to `--set kubeletDir=/var/snap/microk8s/common/var/lib/kubelet`.


[#29]: https://github.com/stackabletech/listener-operator/pull/29
[#34]: https://github.com/stackabletech/listener-operator/pull/34
[#45]: https://github.com/stackabletech/listener-operator/pull/45
