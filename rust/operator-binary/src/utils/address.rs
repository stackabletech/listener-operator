use stackable_operator::{crd::listener, k8s_openapi::api::core::v1::Node};

/// The primary addresses of an entity, for each type of address.
#[derive(Debug, Clone, Copy)]
pub struct AddressCandidates<'a> {
    pub ip: Option<&'a str>,
    pub hostname: Option<&'a str>,
}

impl<'a> AddressCandidates<'a> {
    /// Tries to pick the preferred [`listener::v1alpha1::AddressType`], falling back if it is not available.
    pub fn pick(
        &self,
        preferred_address_type: listener::v1alpha1::AddressType,
    ) -> Option<(&'a str, listener::v1alpha1::AddressType)> {
        let ip = self.ip.zip(Some(listener::v1alpha1::AddressType::Ip));
        let hostname = self
            .hostname
            .zip(Some(listener::v1alpha1::AddressType::Hostname));
        match preferred_address_type {
            listener::v1alpha1::AddressType::Ip => ip.or(hostname),
            listener::v1alpha1::AddressType::Hostname => hostname.or(ip),
        }
    }
}

/// Try to guess the primary addresses of a Node, which it is expected that external clients should be able to reach it on
pub fn node_primary_addresses(node: &'_ Node) -> AddressCandidates<'_> {
    let addrs = node
        .status
        .as_ref()
        .and_then(|s| s.addresses.as_deref())
        .unwrap_or_default();

    AddressCandidates {
        ip: addrs
            .iter()
            .find(|addr| addr.type_ == "ExternalIP")
            .or_else(|| addrs.iter().find(|addr| addr.type_ == "InternalIP"))
            .map(|addr| addr.address.as_str()),
        hostname: addrs
            .iter()
            .find(|addr| addr.type_ == "Hostname")
            .map(|addr| addr.address.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use stackable_operator::{
        crd::listener,
        k8s_openapi::api::core::v1::{Node, NodeAddress, NodeStatus},
    };

    use super::node_primary_addresses;

    #[test]
    fn node_with_only_ips_primary_address_returns_external_ip() {
        let node = node_from_addresses(vec![("InternalIP", "10.1.2.3"), ("ExternalIP", "1.2.3.4")]);
        let node_primary_address = node_primary_addresses(&node);
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Ip),
            Some(("1.2.3.4", listener::v1alpha1::AddressType::Ip))
        );
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Hostname),
            Some(("1.2.3.4", listener::v1alpha1::AddressType::Ip))
        );
    }

    #[test]
    fn node_with_only_hostname_primary_address_returns_hostname() {
        let node = node_from_addresses(vec![
            ("Hostname", "first-hostname"),
            ("Hostname", "second-hostname"),
        ]);
        let node_primary_address = node_primary_addresses(&node);
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Ip),
            Some(("first-hostname", listener::v1alpha1::AddressType::Hostname))
        );
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Hostname),
            Some(("first-hostname", listener::v1alpha1::AddressType::Hostname))
        );
    }

    #[test]
    fn node_with_hostname_and_ips_primary_address() {
        let node = node_from_addresses(vec![
            ("Hostname", "node-0"),
            ("ExternalIP", "1.2.3.4"),
            ("InternalIP", "10.1.2.3"),
        ]);
        let node_primary_address = node_primary_addresses(&node);
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Ip),
            Some(("1.2.3.4", listener::v1alpha1::AddressType::Ip))
        );
        assert_eq!(
            node_primary_address.pick(listener::v1alpha1::AddressType::Hostname),
            Some(("node-0", listener::v1alpha1::AddressType::Hostname))
        );
    }

    fn node_from_addresses<'a>(addresses: impl IntoIterator<Item = (&'a str, &'a str)>) -> Node {
        Node {
            status: Some(NodeStatus {
                addresses: Some(
                    addresses
                        .into_iter()
                        .map(|(ty, addr)| NodeAddress {
                            type_: ty.to_string(),
                            address: addr.to_string(),
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}
