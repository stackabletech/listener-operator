# Stackable Load Balancer Operator

A CSI provider intended to provide an abstract way to expose a single Pod to the outside network,
while hiding details about the cluster from the application developer.

This is intended to be used by services that perform their own clustering and replica discovery,
and require external clients to connect to the correct replica. For example, this would be suitable
for HDFS DataNodes and Kafka listener connections, but not so much for HDFS NameNodes, or Kafka bootstrap
addresses.

## Usage

### Running

`nix run -f. tilt up`

### LoadBalancerClass

`LoadBalancerClass` objects are used by cluster administrators to define a policy for how incoming connections
should be handled. For example, a small on-prem cluster might prefer to use `NodePort` services (because they don't
require BGP peering or ARP spoofing, at the cost of making each instance "sticky" to its initial K8s Node), while a
managed cloud cluster might prefer `LoadBalancer`.

### Pods

Pods are exposed by mounting a `PersistentVolume` with the `storageClassName` of `lb.stackable.tech` into them.
This can either be created as a `volumeClaimTemplate` of a `StatefulSet` (ensuring that each network identity will be
persistent for each replica identity, even across pod replacements) or an `ephemeral` pod `Volume` (in which case the
network identity will be recreated from scratch for every Pod).

The `LoadBalancerClass` is specified using the `lb.stackable.tech/lb-class` annotation on the `PersistentVolumeClaim`.

The mounted volume will contain the file `address` (containing the external address of the `Pod`), as well as
`ports/{port-name}` (containing the port number that the port is accessible on `address` from, which may or may not
be the same as the `containerPort`).
