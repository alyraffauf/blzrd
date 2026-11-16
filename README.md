# blzrd

A blazing fast deployment tool for distributed NixOS, Darwin, and system-manager fleets. Useful for quickly and efficiently deploying the same flake revision to multiple machines in parallel.

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
- `boot`: Set the new configuration as the boot default without activating. NixOS only.

### Options

- `--flake <url>`: Flake URL or path (env: `FLAKE`, default: `.`). Global; can be placed before or after the subcommand.
- `--build-host <host>`: Build closures on this remote host instead of locally (default: `localhost`).
- `--accept-new-host-keys`: Accept and record previously unknown SSH host keys. Changed keys are still rejected.
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

### Sample `blzrd.nodes`

blzrd is configured with a Flake output containing an attrset that defines a set of deployment jobs. Outputs can be declared in the same Flake or in an upstream Flake.

The deployment user must be specified. Non-root users require passwordless `sudo` access.

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

    webserver = {
      output = self.systemConfigs.webserver;
      user = "deploy";
      type = "system-manager";
    };
  };
}
```

### Flake validation

For module and library integration, see
[the flake integration guide](docs/flake-integration.md).

### System-manager

Set `type = "system-manager"` explicitly; Linux outputs otherwise default to
NixOS. Use the derivation returned by system-manager's `lib.makeSystemConfig`
as the node's `output`.

`switch` runs the output's `bin/register-profile`, then runs `bin/activate`
in a transient systemd unit and waits for its result. Registration manages
system-manager's own profile and GC root. `boot` is not supported.

Targets need a system-manager-supported Linux system with systemd and Nix.
`systemd-run` must be available to root or through sudo, and `systemctl` must
be on the SSH user's PATH. The deployment user needs root access or
passwordless sudo, plus Nix daemon permission to import the copied closures.
No separately installed system-manager CLI is required.

## Limitations

- Requires SSH root access or the ability to escalate privileges with sudo without password entry. It won't prompt for a password, it just fails.
- Does not (yet) support other forms of Nix profiles, such as home-manager.

## License

This project is licensed under the GNU General Public License v3.0.

## Contribution

Contributions are welcome! Please open issues or submit pull requests for any improvements you make or bugs you encounter.

## Contact

You can reach me at aly @ aly dot codes.
