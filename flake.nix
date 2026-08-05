{
  description = "nahida — a small coding agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # rust toolchain
            rustc
            cargo
            rustfmt
            clippy
            # the agent shells out to bash; make sure one is on PATH
            bashInteractive
            # inspecting the wire format while working on nahida-llm
            curl
            jq
          ];
        };
      });
}
