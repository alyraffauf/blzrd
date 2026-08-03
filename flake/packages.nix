_: {
  perSystem = {
    pkgs,
    self',
    ...
  }: {
    packages = rec {
      blzrd = pkgs.rustPlatform.buildRustPackage {
        pname = "blzrd";
        src = ../.;

        cargoLock.lockFile = ../Cargo.lock;

        nativeBuildInputs = with pkgs; [
          makeWrapper
        ];

        postInstall = ''
          wrapProgram $out/bin/blzrd \
            --prefix PATH : ${pkgs.lib.makeBinPath [pkgs.nix-eval-jobs]}
        '';

        version =
          if self' ? shortRev
          then "git-${self'.shortRev}"
          else "dev";
      };

      blzrd-lix = pkgs.rustPlatform.buildRustPackage {
        pname = "blzrd";
        src = ../.;

        cargoLock.lockFile = ../Cargo.lock;

        nativeBuildInputs = with pkgs; [
          makeWrapper
        ];

        postInstall = ''
          wrapProgram $out/bin/blzrd \
            --prefix PATH : ${pkgs.lib.makeBinPath [pkgs.lixPackageSets.latest.nix-eval-jobs]}
        '';

        version =
          if self' ? shortRev
          then "git-${self'.shortRev}"
          else "dev";
      };

      default = blzrd;
    };
  };
}
