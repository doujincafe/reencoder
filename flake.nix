{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
  };
  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      top@{ config, ... }:
      {
        imports = [ inputs.treefmt-nix.flakeModule ];
        systems = [
          "aarch64-darwin"
          "aarch64-linux"
          "x86_64-darwin"
          "x86_64-linux"
        ];
        perSystem =
          { pkgs, ... }:
          let
            naersk' = pkgs.callPackage inputs.naersk { };
            nativeBuildInputs' = with pkgs; [
              cargo
              cmake
              flac
              rustc
              sqlite
            ];
          in
          {
            packages.default = naersk'.buildPackage {
              src = ./.;
              nativeBuildInputs = nativeBuildInputs';
            };

            devShells.default = pkgs.mkShell {
              nativeBuildInputs = nativeBuildInputs';
              packages = with pkgs; [
                nixd
                nixfmt
                rust-analyzer
                rustfmt
                clippy
              ];
            };

            treefmt = {
              projectRootFile = "flake.nix";
              programs = {
                nixfmt.enable = true;
                rustfmt.enable = true;
                taplo.enable = true;
              };
            };
          };
      }
    );
}

#   outputs =
#     {
#       self,
#       flake-utils,
#       naersk,
#       nixpkgs,
#       treefmt-nix,
#       ...
#     }:
#     flake-utils.lib.eachDefaultSystem (
#       system:
#       let
#         pkgs = (import nixpkgs) { inherit system; };

#         naersk' = pkgs.callPackage naersk { };

#         treefmtEval = treefmt-nix.lib.evalModule pkgs ./treefmt.nix;

#       in
#       {
#         # For `nix build` & `nix run`:
#         packages.default = naersk'.buildPackage {
#           src = ./.;
#           nativeBuildInputs = with pkgs; [
#             cmake
#             flac
#             sqlite
#           ];
#         };

#         # For `nix develop`:
#         devShells.${system}.default = pkgs.mkShell {
#           nativeBuildInputs = with pkgs; [
#             cargo
#             rustc
#           ];
#           packages = with pkgs; [
#             nixd
#             nixfmt
#             rust-analyzer
#             rustfmt
#             clippy
#           ];
#         };
#         formatter = treefmtEval.config.build.wrapper;
#         checks = {
#           formatting = treefmtEval.config.build.check self;
#         };
#       }
#     );
# }
