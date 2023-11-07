{
  description = "FlexION development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config.allowUnfree = true;
    };
    pythonEnv = pkgs.python311.withPackages (p: with p; [
      horizon-eda
      pyexcel
    ]);
  in {
    devShells."${system}".default = pkgs.mkShell {
      packages = [
        pkgs.horizon-eda
        pythonEnv
      ];
    };
    apps."${system}" = {
      horizon-eda = {
        type = "app";
        program = "${pkgs.horizon-eda}/bin/horizon-eda";
      };
    };
  };
}
