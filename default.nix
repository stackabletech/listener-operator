{ sources ? import ./nix/sources.nix # managed by https://github.com/nmattia/niv
, nixpkgs ? sources.nixpkgs
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
    };
  }
, dockerName ? "docker.stackable.tech/sandbox/listener-operator"
, dockerTag ? null
}:
rec {
  build = cargo.rootCrate.build;
  crds = pkgs.runCommand "listener-operator-crds.yaml" {}
  ''
    ${build}/bin/stackable-listener-operator crd > $out
  '';

  dockerImage = pkgs.dockerTools.streamLayeredImage {
    name = dockerName;
    tag = dockerTag;
    contents = [ pkgs.bashInteractive pkgs.coreutils pkgs.util-linuxMinimal ];
    config = {
      Cmd = [ (build+"/bin/stackable-listener-operator") "run" ];
    };
  };
  docker = pkgs.linkFarm "listener-operator-docker" [
    {
      name = "load-image";
      path = dockerImage;
    }
    {
      name = "ref";
      path = pkgs.writeText "${dockerImage.name}-image-tag" "${dockerImage.imageName}:${dockerImage.imageTag}";
    }
    {
      name = "image-repo";
      path = pkgs.writeText "${dockerImage.name}-repo" dockerImage.imageName;
    }
    {
      name = "image-tag";
      path = pkgs.writeText "${dockerImage.name}-tag" dockerImage.imageTag;
    }
    {
      name = "crds.yaml";
      path = crds;
    }
  ];

  crate2nix = pkgs.crate2nix;
  tilt = pkgs.tilt;
}
