{lib}: let
  isDerivation = value:
    builtins.isAttrs value && (value.type or null) == "derivation";

  validateNode = name: node:
    if !builtins.isAttrs node
    then ["node '${name}': must be an attribute set"]
    else let
      output = node.output or null;
      user = node.user or null;
      hostname = node.hostname or null;
      type = node.type or null;
    in
      (lib.optional (!isDerivation output)
        "node '${name}': output must be a derivation")
      ++ (lib.optional
        (!(builtins.isString user) || user == "")
        "node '${name}': user must be a non-empty string")
      ++ (lib.optional
        (!(builtins.isNull hostname || builtins.isString hostname)
          || (builtins.isString hostname && hostname == ""))
        "node '${name}': hostname must be null or a non-empty string")
      ++ (lib.optional
        (!(builtins.isNull type
          || (builtins.isString type && lib.elem type ["darwin" "nixos"])))
        "node '${name}': type must be null, darwin, or nixos");

  validateNodes = nodes:
    if !builtins.isAttrs nodes
    then ["blzrd.nodes must be an attribute set"]
    else lib.concatLists (lib.mapAttrsToList validateNode nodes);

  assertValidNodes = nodes: let
    validationErrors = validateNodes nodes;
  in
    if validationErrors == []
    then nodes
    else
      throw ''
        Invalid blzrd.nodes configuration:
        ${lib.concatStringsSep "\n" validationErrors}
      '';

  checkNodes = {
    pkgs,
    nodes,
  }:
    builtins.seq (assertValidNodes nodes) (pkgs.runCommand "blzrd-nodes-check" {} ''
      touch "$out"
    '');
in {
  inherit assertValidNodes checkNodes validateNodes;
}
