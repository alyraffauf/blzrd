{
  config,
  lib,
  ...
}: let
  cfg = config.blzrd;
  blzrdLib = import ./lib.nix {inherit lib;};
  validatedNodes = blzrdLib.assertValidNodes cfg.nodes;
in {
  options.blzrd = {
    nodes = lib.mkOption {
      default = {};
      description = "Nodes managed by blzrd.";
      type = lib.types.attrsOf (lib.types.submodule {
        freeformType = lib.types.attrsOf lib.types.raw;

        options = {
          output = lib.mkOption {
            description = "The system derivation to deploy.";
            type = lib.types.raw;
          };

          hostname = lib.mkOption {
            default = null;
            description = "The SSH hostname; defaults to the node name in blzrd.";
            type = lib.types.nullOr lib.types.str;
          };

          type = lib.mkOption {
            default = null;
            description = "The activation backend, either nixos or darwin.";
            type = lib.types.nullOr (lib.types.enum ["darwin" "nixos"]);
          };

          user = lib.mkOption {
            description = "The SSH and deployment user.";
            type = lib.types.str;
          };
        };
      });
    };

    checks.enable =
      lib.mkEnableOption "the blzrd node flake check"
      // {
        default = true;
      };
  };

  config = {
    flake.blzrd.nodes = validatedNodes;

    perSystem = {pkgs, ...}:
      lib.mkIf cfg.checks.enable {
        checks.blzrd-nodes = blzrdLib.checkNodes {
          inherit pkgs;
          inherit (cfg) nodes;
        };
      };
  };
}
