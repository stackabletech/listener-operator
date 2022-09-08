# Helm Chart for Stackable Operator for Stackable Listener Operator

This Helm Chart can be used to install Custom Resource Definitions and the Stackable Listener Operator.

## Requirements

- Create a [Kubernetes Cluster](../Readme.md)
- Install [Helm](https://helm.sh/docs/intro/install/)

## Install the Stackable Operator for Stackable Load Balancer Operator

```bash
# From the root of the operator repository
make compile-chart

helm install lb-operator deploy/helm/lb-operator
```

## Usage of the CRDs

The usage of this operator and its CRDs is described in the [documentation](https://docs.stackable.tech/lb-operator/index.html)

The operator has example requests included in the [`/examples`](https://github.com/stackabletech/lb-operator/tree/main/examples) directory.

## Links

https://github.com/stackabletech/lb-operator
