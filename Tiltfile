default_registry("docker.stackable.tech/sandbox")

custom_build(
    'docker.stackable.tech/sandbox/listener-operator',
    'nix run -f . crate2nix generate && nix-build . -A docker --argstr dockerName "${EXPECTED_REGISTRY}/listener-operator" && ./result/load-image | docker load',
    deps=['rust', 'Cargo.toml', 'Cargo.lock', 'default.nix', "nix", 'build.rs', 'vendor'],
    # ignore=['result*', 'Cargo.nix', 'target', *.yaml],
    outputs_image_ref_to='result/ref',
)

# Load the latest CRDs from Nix
watch_file('result')
if os.path.exists('result'):
   k8s_yaml('result/crds.yaml')

# Exclude stale CRDs from Helm chart, and apply the rest
helm_crds, helm_non_crds = filter_yaml(
   helm(
      'deploy/helm/listener-operator',
      name='listener-operator',
      set=[
         'image.repository=docker.stackable.tech/sandbox/listener-operator',
      ],
   ),
   api_version = "^apiextensions\\.k8s\\.io/.*$",
   kind = "^CustomResourceDefinition$",
)
k8s_yaml(helm_non_crds)

# Load examples
k8s_yaml('examples/nginx-nodeport.yaml')
k8s_yaml('examples/nginx-lb.yaml')
k8s_yaml('examples/nginx-preprovisioned-lb.yaml')
