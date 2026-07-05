package cli

import (
	"fmt"
	"io"
	"os"
)

const Version = "0.1.0"

func Main(args []string) int {
	if err := Run(args); err != nil {
		fmt.Fprintf(os.Stderr, "graft: %v\n", err)
		return 1
	}
	return 0
}

func Run(args []string) error {
	if len(args) == 0 {
		Usage(os.Stdout)
		return nil
	}

	switch args[0] {
	case "--help", "-h", "help":
		Usage(os.Stdout)
		return nil
	case "--version", "version":
		fmt.Println(Version)
		return nil
	case "config":
		return runConfig(args[1:])
	case "inspect":
		return inspectYAML(args[1:])
	case "up":
		return graftUp(args[1:])
	case "render":
		return renderYAML(args[1:])
	case "render-nixos":
		return renderNixOS(args[1:])
	case "render-nixos-units":
		return renderNixOSUnits(args[1:])
	case "prepare-rootfs":
		return prepareRootfs(args[1:])
	case "run":
		return graftRun(args[1:])
	case "run-rootfs":
		return runRootfs(args[1:])
	case "nix-bake":
		return nixBake(args[1:])
	case "start":
		return graftStart(args[1:])
	case "attach":
		return graftAttach(args[1:])
	case "list":
		return graftList(args[1:])
	case "logs":
		return graftLogs(args[1:])
	case "stop":
		return graftStop(args[1:])
	case "diff":
		return graftDiff(args[1:])
	case "promote":
		return graftPromote(args[1:])
	case "reset":
		return graftReset(args[1:])
	default:
		// Try as a named container shortcut: find <name>.toml in configRoot.
		return graftStartOrAttach(args[0])
	}
}

func Usage(w io.Writer) {
	_, _ = fmt.Fprint(w, `graft

Usage:
  graft config path
  graft config init [path]
  graft config show [path]
  graft up [file.toml]
  graft inspect <file.toml>
  graft render <file.toml>
  graft render-nixos <file.toml> <rootfs> <container-name>
  graft render-nixos-units <file.toml> <container-name> <out-dir>
  graft prepare-rootfs <dir>
  graft run <file.toml>
  graft run-rootfs -- <command> [args...]
  graft nix-bake <dir>
  graft start <file.toml>
  graft attach <container-name>
  graft list
  graft logs [-f] <container-name>
  graft stop <container-name>
  graft diff [--host <host>] <container-name>
  graft promote [--host <host>] <container-name>
  graft reset [--host <host>] <container-name>
  graft <name>                         (start-or-attach from configRoot/<name>.toml)

Commands:
  config         Manage the no-op example config
  up             Run a TOML config, autodetecting one if omitted
  inspect        Inspect a TOML config and print JSON metadata
  render         Render a minimal TOML config to Quadlet text
  render-nixos   Render a TOML config with concrete NixOS store paths
  render-nixos-units  Render all Quadlet units with concrete NixOS store paths
  prepare-rootfs Create a writable minimal rootfs (used by managed units at start)
  run            Run a TOML config through a temporary Quadlet unit
  run-rootfs     Run a command through a temporary rootfs Quadlet unit
  nix-bake       Prefetch npm deps hash and emit a buildNpmPackage Nix snippet
  start          Start a container detached (writes Quadlet unit, daemon-reload, systemctl start)
  attach         Attach to a running container's tmux session
  list           List running graft-managed containers
  logs           Show container logs (-f to follow)
  stop           Stop a running container and remove its runtime unit
  diff           Show diff of shadow mount(s) for a running session [--host for remote]
  promote        Promote shadow mount changes back to the host [--host for remote]
  reset          Reset shadow mount(s) from original host source [--host for remote]
  <name>         Start (if not running) and attach to configRoot/<name>.toml
`)
}
