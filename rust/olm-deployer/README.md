# Description

This is an deployment helper for the Operator Lifecycle Manager which is usually present on OpenShift environments.

It is needed to work around various OLM restrictions.

What it does:

- creates Security Context Constraints just for this operator (maybe remove in the future)
- installs the Deployment and DaemonSet objects
- installs the operator service
- installs the CSI driver and storage classes
- assigns it's own deployment as owner of all the namespaced objects to ensure proper cleanup
- patches the environment of all workload containers with any custom values provided in the Subscription object
- patches the resources of all workload containers with any custom values provided in the Subscription object
- patches the tolerations of all workload pods with any custom values provided in the Subscription object

## Usage

Users do not need to interact with the OLM deployer directly.
