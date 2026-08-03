_: {
  perSystem = {
    config,
    lib,
    pkgs,
    ...
  }: {
    devShells.default = pkgs.mkShell {
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
