{ nixpkgs ? <nixpkgs>
, pkgs ? import nixpkgs {}
, cargo ? import ./Cargo.nix {
    inherit nixpkgs pkgs; release = false;
    defaultCrateOverrides = pkgs.defaultCrateOverrides // {
      prost-build = attrs: {
        buildInputs = [ pkgs.protobuf ];
      };
      tonic-reflection = attrs: {
        buildInputs = [ pkgs.rustfmt ];
      };
      stackable-lb-operator = attrs: {
        buildInputs = [ pkgs.rustfmt ];
      };
    };
  }
, dockerRegistry ? "docker.stackable.tech"
, dockerRepo ? "${dockerRegistry}/teozkr/lb-operator"
, dockerTag ? "latest"
}:
rec {
  build = cargo.rootCrate.build;
  crds = pkgs.runCommand "lb-provisioner-crds.yaml" {}
  ''
    ${build}/bin/stackable-lb-operator crd > $out
  '';

  dockerImage = pkgs.dockerTools.streamLayeredImage {
    name = dockerRepo;
    tag = dockerTag;
    contents = [ pkgs.bashInteractive pkgs.coreutils pkgs.util-linuxMinimal ];
    config = {
      Cmd = [ (build+"/bin/stackable-lb-operator") "run" ];
    };
  };
  docker = pkgs.linkFarm "lb-provisioner-docker" [
    {
      name = "load-image";
      path = dockerImage;
    }
    {
      name = "ref";
      path = pkgs.writeText "${dockerImage.name}-image-tag" "${dockerImage.imageName}:${dockerImage.imageTag}";
    }
    {
      name = "crds.yaml";
      path = crds;
    }
  ];

  crate2nix = pkgs.crate2nix;
  tilt = pkgs.tilt;
}
