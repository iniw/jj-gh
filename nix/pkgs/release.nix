{
  pkgs,
  rustToolchain,
}:
pkgs.writeShellApplication {
  name = "release";
  runtimeInputs = [
    rustToolchain
    pkgs.release-plz
    pkgs.cargo-semver-checks
    pkgs.git
  ];
  text = ''
    release-plz release-pr --git-token "$GITHUB_TOKEN" "$@"
    release-plz release --git-token "$GITHUB_TOKEN" "$@"
  '';
}
