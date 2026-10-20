{ pkgs }:
{
  projectRootFile = "flake.nix";
  programs = {
    rustfmt.enable = true;
    prettier = {
      enable = true;
      includes = [
        "*.md"
        "*.gql"
        "*.json"
      ];
      excludes = [ "CHANGELOG.md" ];
    };
    taplo.enable = true;
    yamlfmt.enable = true;
    nixfmt.enable = true;
    actionlint.enable = true;
  };
  settings.formatter.tombi = {
    command = "${pkgs.tombi}/bin/tombi";
    options = [
      "format"
      "--offline"
    ];
    includes = [ "*.toml" ];
  };
}
