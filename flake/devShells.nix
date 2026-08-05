_: {
  perSystem = {
    config,
    lib,
    pkgs,
    ...
  }: {
    devShells.default = pkgs.mkShell {
      env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

      packages = with pkgs;
        [
          cargo
          clippy
          nixd
          rust-analyzer
          rustc
          rustfmt
        ]
        ++ lib.attrValues config.treefmt.build.programs;
    };
  };
}
