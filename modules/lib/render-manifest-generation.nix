{
  lib,
  pkgs,
  package,
  materialised,
  producer,
  hostId,
  target,
  manager,
  workerApiRange,
  requiredBackend,
  name,
}:

let
  input = import ./lower-manifest-input.nix {
    inherit
      lib
      pkgs
      materialised
      producer
      hostId
      target
      manager
      workerApiRange
      requiredBackend
      ;
  };

  generation = pkgs.runCommand name { } ''
    set -euo pipefail
    umask 0022

    ${lib.getExe' package "graft-manifest-render"} ${input.file} "$out"

    test -f "$out/manifest.json"
    test ! -L "$out/manifest.json"
    test -f "$out/endpoint.json"
    test ! -L "$out/endpoint.json"
    test "$(find "$out" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)" = $'endpoint.json\nmanifest.json'

    chmod 0444 "$out/manifest.json" "$out/endpoint.json"
    chmod 0555 "$out"
  '';
in
{
  inherit generation input;
}
