# blzrd

A blazing fast deployment tool for distributed NixOS and Darwin fleets. Useful for quickly and efficiently deploying the same flake revision to multiple machines in parallel.

A Rust port and continuance of [`nynx`](https://github.com/alyraffauf/nynx).

## Usage

Build the tool using Nix:

```
nix build .#blzrd
```

Run deployments:

```
blzrd switch --flake <flake-url>
```

### Commands

- `switch`: Activate the new configuration and make it the boot default.
- `boot`: Set the new configuration as the boot default without activating.
- `list`: List nodes declared in the flake without deploying anything.

### Options

- `--flake <url>`: Flake URL or path (env: `FLAKE`, default: `.`). Global; can be placed before or after the subcommand.
- `--build-host <host>`: Build closures on this remote host instead of locally (default: `localhost`).
- `--skip <nodes>`: Comma-separated nodes to skip.
- positional `nodes`: Nodes to deploy (default: all). Accepts a comma- or space-separated list.
- `RUST_LOG=debug`: Enable debug output showing every subprocess command and its output.

### Example

Deploy every node in a flake:

```
blzrd switch --flake github:alyraffauf/infra
```

Deploy specific nodes (positional):

```
blzrd switch --flake github:alyraffauf/infra server,workstation
```

List what's in a flake without deploying:

```
blzrd list --flake github:alyraffauf/infra
```

### Sample `blzrd.nodes`

blzrd is configured with a Flake output containing an attrset that defines a set of deployment jobs. Outputs can be declared in the same Flake or in an upstream Flake.

```nix
{
  blzrd.nodes = {
    evergrande = {
      hostname = "evergrande"; # Will be assumed from deployment name if not specified.
      output = self.inputs.nixcfg.nixosConfigurations.evergrande.config.system.build.toplevel;
      type = "nixos";
      user = "root";
    };

    fortree = {
      output = self.darwinConfigurations.fortree.config.system.build.toplevel;
      user = "aly";
      type = "darwin";
    };
  };
}
```

## Limitations

- Requires SSH root access or the ability to escalate privileges with sudo without password entry. It won't prompt for a password, it just fails.
- Does not (yet) support other forms of Nix profiles, such as home-manager.

## License

This project is licensed under the GNU General Public License v3.0.

## Contribution

Contributions are welcome! Please open issues or submit pull requests for any improvements you make or bugs you encounter.

## Contact

You can reach me at aly @ aly dot codes.
