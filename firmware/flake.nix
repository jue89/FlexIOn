{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, rust-overlay, flake-utils }: flake-utils.lib.eachDefaultSystem (system: let
  	pkgs = import nixpkgs {
      inherit system;
      overlays = [ (import rust-overlay) ];
    };
    craneLib = (crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.stable.latest.default.override {
      extensions = [ "rust-src" "llvm-tools" ];
      targets = [ "thumbv7m-none-eabi" ];
    });
  in {
  	devShells.default = craneLib.devShell {
      packages = [
        pkgs.probe-rs-tools
        pkgs.rust-analyzer
        pkgs.cargo-binutils
        pkgs.cargo-bloat
        pkgs.cargo-expand
        pkgs.git
      ];
    };
  });
}
