{
  pkgs,
  craneLib,
  commonArgs,
  cargoArtifacts,
  treefmt,
}:
let
  callPackage = pkgs.lib.callPackageWith {
    inherit
      pkgs
      craneLib
      commonArgs
      cargoArtifacts
      treefmt
      ;
  };

  gen-docs = callPackage ./gen-docs.nix { };
  gen-manpage = callPackage ./gen-manpage.nix { };
  jj-gh = callPackage ./jj-gh.nix { };
  update-docs = callPackage ./update-docs.nix { inherit gen-docs; };
in
{
  inherit
    jj-gh
    update-docs
    ;

  fetch-schema-app = callPackage ./fetch-schema.nix { };
  release-app = callPackage ./release.nix { };

  packages = {
    inherit gen-docs gen-manpage;
    default = jj-gh;
    dev = callPackage ./jj-gh-dev.nix { };
  };
}
