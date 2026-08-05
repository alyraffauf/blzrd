# Flake integration

blzrd exposes two ways to validate a `blzrd.nodes` definition through
`nix flake check`:

- a flake-parts module that wires the check automatically;
- a reusable Nix library for flakes that want to wire the check themselves.

Validation happens during Nix evaluation. It does not build system closures,
connect to nodes, or deploy anything.

## Flake-parts module

Import `inputs.blzrd.flakeModule` and define `blzrd.nodes` in the flake-parts
configuration:

```nix
{
  inputs.blzrd.url = "github:alyraffauf/blzrd";

  outputs = inputs @ { self, flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.blzrd.flakeModule ];

      blzrd.nodes = {
        server = {
          output = self.nixosConfigurations.server.config.system.build.toplevel;
          user = "root";
          type = "nixos";
        };
      };
    };
}
```

The module exposes `blzrd.nodes` as a flake output and adds
`checks.<system>.blzrd-nodes`. The check is enabled by default. To disable it:

```nix
blzrd.checks.enable = false;
```

## Library API

Flakes that do not use flake-parts can use `inputs.blzrd.lib` directly:

```nix
{
  inputs = {
    blzrd.url = "github:alyraffauf/blzrd";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, blzrd, nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      nodes = {
        server = {
          output = self.nixosConfigurations.server.config.system.build.toplevel;
          user = "root";
          type = "nixos";
        };
      };

      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      blzrd.nodes = nodes;

      checks = forAllSystems (system: {
        blzrd-nodes = blzrd.lib.checkNodes {
          pkgs = nixpkgs.legacyPackages.${system};
          inherit nodes;
        };
      });
    };
}
```

The library provides three functions:

- `validateNodes nodes` returns a list of validation error strings.
- `assertValidNodes nodes` returns `nodes` or throws with all validation errors.
- `checkNodes { pkgs, nodes }` returns a derivation suitable for
  `checks.<system>.<name>`.

Each node must provide a derivation-valued `output` and a non-empty `user`.
`hostname` is optional and may be null; `type` is optional and may be null,
`"nixos"`, or `"darwin"`.
