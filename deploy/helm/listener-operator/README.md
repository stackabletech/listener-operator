<!-- markdownlint-disable MD034 -->
# Helm Chart for Stackable Operator for Stackable Listener Operator

This Helm Chart can be used to install Custom Resource Definitions and the Operator for Stackable Listener Operator provided by Stackable.

## Requirements

- Create a [Kubernetes Cluster](../Readme.md)
- Install [Helm](https://helm.sh/docs/intro/install/)

## Install the Stackable Operator for Stackable Listener Operator

```bash
# From the root of the operator repository
make compile-chart

helm install listener-operator deploy/helm/listener-operator
```

## Usage of the CRDs

The usage of this operator and its CRDs is described in the [documentation](https://docs.stackable.tech/listener-operator/index.html)

The operator has example requests included in the [`/examples`](https://github.com/stackabletech/listener-operator/tree/main/examples) directory.

## Links

<https://github.com/stackabletech/listener-operator>
