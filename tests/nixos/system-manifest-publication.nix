{ pkgs, graftPackage }:

let
  hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
  publisher = pkgs.lib.getExe' graftPackage "graft-manifest-publish-system";
  producerVersion = graftPackage.version;
  manifestQuery = pkgs.lib.escapeShellArg ''.hostId == "${hostId}" and .target == "system" and .manager == "system" and .producer.buildId == "source" and .generationId == .manifestDigest and .workloadCount == 0 and .workloads == [] and (has("uid") | not)'';
  endpointQuery = pkgs.lib.escapeShellArg ''.hostId == "${hostId}" and .target == "system" and .manager == "system" and .producer.buildId == "source" and .generationId == .manifestDigest and (has("uid") | not)'';
  manifestCommand = "jq -e ${manifestQuery} /etc/graft/current/manifest.json";
  endpointCommand = "jq -e ${endpointQuery} /etc/graft/current/endpoint.json";
in
{
  name = "graft-system-manifest-publication";

  nodes.machine = {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      inherit hostId;
    };

    virtualisation = {
      diskSize = 1024;
      memorySize = 768;
    };

    environment.systemPackages = [
      pkgs.jq
      pkgs.util-linux
    ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    generation = machine.succeed("readlink /etc/graft/current").strip()
    graft_gid = machine.succeed("getent group graft | cut -d: -f3").strip()

    def publish_command(target):
        return f"${publisher} {target} --graft-gid {graft_gid} --producer-name graft --producer-version ${producerVersion} --producer-build-id source"

    assert generation.startswith("/nix/store/")
    machine.succeed(f"test ! -L {generation}")
    machine.succeed(f"test $(dirname {generation}) = /nix/store")

    machine.succeed("test $(stat -c '%a:%U:%G:%F' /etc/graft) = 750:root:graft:directory")
    machine.succeed("test -L /etc/graft/current; test $(stat -c '%U:%G' /etc/graft/current) = root:root")
    machine.succeed("test $(stat -c '%a:%U:%G:%F' /run/graft) = 755:root:root:directory")
    machine.succeed("test $(stat -c '%a:%U:%G:%F' /run/graft/system) = 750:root:graft:directory")
    machine.succeed("test -f /run/graft/system/activation.lock; test $(stat -c '%a:%U:%G:%h' /run/graft/system/activation.lock) = 600:root:root:1")
    machine.succeed(f"test $(stat -c '%a:%U:%G:%F' {generation}) = 555:root:root:directory")
    machine.succeed(f"test -f {generation}/manifest.json; test $(stat -c '%a:%U:%G' {generation}/manifest.json) = 444:root:root")
    machine.succeed(f"test -f {generation}/endpoint.json; test $(stat -c '%a:%U:%G' {generation}/endpoint.json) = 444:root:root")
    machine.succeed(f"test \"$(find {generation} -mindepth 1 -maxdepth 1 -printf '%f\\n' | sort)\" = $'endpoint.json\\nmanifest.json'")

    machine.succeed(${builtins.toJSON manifestCommand})
    machine.succeed(${builtins.toJSON endpointCommand})
    machine.succeed("test $(jq -r .generationId /etc/graft/current/manifest.json) = $(jq -r .generationId /etc/graft/current/endpoint.json)")
    machine.succeed("test $(jq -r .manifestDigest /etc/graft/current/manifest.json) = $(jq -r .manifestDigest /etc/graft/current/endpoint.json)")

    inode_before = machine.succeed("stat -c %i /etc/graft/current").strip()
    machine.succeed(publish_command(generation))
    inode_after = machine.succeed("stat -c %i /etc/graft/current").strip()
    assert inode_after == inode_before

    machine.succeed("mkdir /tmp/not-a-store-generation; touch /tmp/not-a-store-generation/{manifest.json,endpoint.json}")
    machine.fail(publish_command("/tmp/not-a-store-generation"))
    assert machine.succeed("readlink /etc/graft/current").strip() == generation

    machine.succeed("mv /etc/graft/current /etc/graft/current.saved; printf malformed > /etc/graft/current")
    machine.fail(publish_command(generation))
    assert machine.succeed("cat /etc/graft/current").strip() == "malformed"
    machine.succeed("rm /etc/graft/current; mv /etc/graft/current.saved /etc/graft/current")

    machine.succeed("rm /run/graft/system/activation.lock; ln -s /etc/passwd /run/graft/system/activation.lock")
    machine.fail(publish_command(generation))
    assert machine.succeed("readlink /etc/graft/current").strip() == generation
    machine.succeed(f"rm /run/graft/system/activation.lock; {publish_command(generation)}")

    machine.succeed("ln /run/graft/system/activation.lock /run/graft/system/activation-linked")
    machine.fail(publish_command(generation))
    assert machine.succeed("readlink /etc/graft/current").strip() == generation
    machine.succeed("rm /run/graft/system/activation-linked")

    machine.succeed("systemd-run --unit=graft-lock-holder --property=Type=exec --quiet flock -x /run/graft/system/activation.lock -c '${pkgs.coreutils}/bin/touch /tmp/graft-lock-held; ${pkgs.coreutils}/bin/sleep 2'")
    machine.wait_until_succeeds("test -e /tmp/graft-lock-held")
    elapsed_ms = int(machine.succeed(f"start=$(date +%s%N); {publish_command(generation)}; echo $((($(date +%s%N) - start) / 1000000))").strip())
    assert elapsed_ms >= 1000
    assert machine.succeed("readlink /etc/graft/current").strip() == generation
  '';
}
