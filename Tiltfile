default_registry('docker.stackable.tech')

custom_build(
    'docker.stackable.tech/teozkr/lb-operator',
    'nix run -f . crate2nix generate && nix-build . -A docker --arg dockerTag null --argstr dockerRegistry k3d-registry.localhost:11337 && ./result/load-image | docker load',
    deps=['rust', 'Cargo.toml', 'Cargo.lock', 'default.nix', 'build.rs', 'vendor'],
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
      'deploy/helm/lb-operator',
      name='lb-operator',
      set=[
         'image.repository=docker.stackable.tech/teozkr/lb-operator',
      ],
   ),
   api_version = "^apiextensions\\.k8s\\.io/.*$",
   kind = "^CustomResourceDefinition$",
)
k8s_yaml(helm_non_crds)

# Load examples
k8s_yaml('examples/nginx-nodeport.yaml')
k8s_yaml('examples/nginx-lb.yaml')
