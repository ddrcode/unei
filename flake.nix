{
  description = "tailorED - my personal vim-like editor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forEachSystem (
        pkgs:
        {
          default = pkgs.mkShell (
            {
              packages =
                with pkgs;
                [
                  rustc
                  cargo
                  clippy
                  rustfmt
                  rust-analyzer
                  cargo-mutants
                  treefmt
                  nixpkgs-fmt
                ]
                ++ lib.optionals stdenv.isDarwin [ libiconv ]
                ++ lib.optionals stdenv.isLinux [ pkg-config ];

              # rust-analyzer needs the stdlib sources
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            }
          );
        }
      );
    };
}
