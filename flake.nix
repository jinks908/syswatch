{
  description = "syswatch — single-host system diagnostics TUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        syswatch = pkgs.callPackage ./package.nix { };
      in
      {
        packages = {
          syswatch = syswatch;
          default = syswatch;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ syswatch ];
          packages = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      }
    );
}
