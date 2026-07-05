package cli

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"

	"github.com/zerodawn1990/graft/internal/config"
	graftproxy "github.com/zerodawn1990/graft/internal/proxy"
	"github.com/zerodawn1990/graft/internal/quadlet"
	graftruntime "github.com/zerodawn1990/graft/internal/runtime"
)

const Version = "0.1.0"

// nixpkgsStorePath is injected at build time by flake.nix via ldflags:
//
//	-X github.com/zerodawn1990/graft/internal/cli.nixpkgsStorePath=${pkgs.path}
//
// It points to the /nix/store path of the pinned nixpkgs used to build graft,
// ensuring that runtime package resolution never touches the host's channel.
var nixpkgsStorePath string

func Main(args []string) int {
	if err := Run(args); err != nil {
		fmt.Fprintf(os.Stderr, "graft: %v\n", err)
		return 1
	}
	return 0
}

func Run(args []string) error {
	// Extract the optional --host flag before dispatching.
	var host string
	host, args = extractFlag(args, "--host")

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
		return graftUp(args[1:], host)
	case "down":
		return graftDown(args[1:], host)
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
	case "attach":
		return graftAttach(args[1:], host)
	case "list":
		return graftList(args[1:], host)
	case "stop":
		return graftStop(args[1:], host)
	case "proxy":
		return runProxy(args[1:])
	case "write-proxy-config":
		return writeProxyConfig(args[1:])
	case "logs":
		return graftLogs(args[1:], host)
	case "diff":
		return graftDiff(args[1:], host)
	case "promote":
		return graftPromote(args[1:], host)
	case "reset":
		return graftReset(args[1:], host)
	default:
		// Start-or-attach shortcut for a pre-deployed instance.
		return graftStartOrAttach(args[0], args[1:], host)
	}
}

func Usage(w io.Writer) {
	_, _ = fmt.Fprint(w, `graft

Usage:
  graft up <instance>                          Start a deployed container
  graft down <instance>                        Stop a running container
  graft attach <instance>                      Attach to the container's tmux session
  graft logs <instance> [--denied]             Show logs (--denied: egress blocks only)
  graft list                                   List running graft-managed containers
  graft stop <instance>                        Stop and remove a transient/dev unit
  graft diff <instance>                        Show diff of session home and shadow mounts
  graft promote <instance> [--path <path>]     Promote shadow mount changes back to host
  graft reset <instance>                       Clear all session data for a container
  graft <instance>                             Start-or-attach shortcut
  graft run <file.toml> --as <instance>        Render and start a transient container (dev path)
  graft inspect <file.toml>                    Print resolved metadata as JSON
  graft render <file.toml>                     Render Quadlet text to stdout
  graft render-nixos <file.toml> <rootfs> <container-name>
  graft render-nixos-units <file.toml> <container-name> <out-dir>
  graft prepare-rootfs <dir>
  graft run-rootfs -- <command> [args...]
  graft nix-bake <dir>
  graft proxy serve                            Run the egress proxy (inside a proxy container)
  graft config path | init [path] | show [path]

Managed-path commands (no TOML reading — operate on pre-deployed instances):
  up           Start a deployed container (systemctl --user start)
  down         Stop a running container (systemctl --user stop)
  attach       Attach to the container's tmux session
  logs         Show service logs (--denied: blocked egress only)
  list         List running graft-managed containers
  stop         Stop and remove a transient/dev unit
  diff         Show diff of session home and shadow mounts
  promote      Promote shadow mount changes back to the host
  reset        Clear all session data for a container
  <instance>   Start-or-attach shortcut

Dev-path commands (reads TOML at runtime):
  run          Render and start a transient container; --as <instance> is required

Plumbing / module support:
  inspect      Print resolved metadata as JSON
  render       Render Quadlet text to stdout
  render-nixos Render with concrete NixOS store paths
  render-nixos-units Render all Quadlet units to a directory
  prepare-rootfs Create a writable minimal rootfs
  run-rootfs   Run a command in a temporary rootfs unit
  nix-bake     Generate a buildNpmPackage Nix snippet

Config management:
  config       Manage the config file (path / init / show)
`)
}

// graftUp starts a pre-deployed container instance via systemctl.
func graftUp(args []string, host string) error {
	if len(args) != 1 {
		return errors.New("up needs: <instance>")
	}
	instance := args[0]
	if err := systemctlScope(host, "start", instance+".service"); err != nil {
		return fmt.Errorf("%w\n  hint: deploy first with nixos-rebuild switch / home-manager switch", err)
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: started %s\n", instance)
	return nil
}

// graftDown stops a pre-deployed container instance via systemctl.
func graftDown(args []string, host string) error {
	if len(args) != 1 {
		return errors.New("down needs: <instance>")
	}
	instance := args[0]
	if err := handleSessionStop(instance, host); err != nil {
		return err
	}
	if err := systemctlScope(host, "stop", instance+".service"); err != nil {
		return err
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: stopped %s\n", instance)
	return nil
}

func renderYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("render needs exactly one TOML file")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := rejectUnresolvedGraphFeatures(file); err != nil {
		return err
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveRuntimeCommand(file.Config.Runtime.Command, file.Config.Runtime.Packages)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                "<runtime-rootfs>",
		FallbackContainerName: "<runtime-container-name>",
		Command:               resolvedArgs,
		Config:                renderConfig,
	})
	if err != nil {
		return err
	}
	fmt.Print(text)
	return nil
}

func renderNixOS(args []string) error {
	if len(args) != 3 {
		return errors.New("render-nixos needs: <file.toml> <rootfs> <container-name>")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveHostStoreCommand(file.Config.Runtime.Command)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                args[1],
		FallbackContainerName: args[2],
		Command:               resolvedArgs,
		Config:                renderConfig,
	})
	if err != nil {
		return err
	}
	fmt.Print(text)
	return nil
}

func renderNixOSUnits(args []string) error {
	if len(args) != 3 {
		return errors.New("render-nixos-units needs: <file.toml> <container-name> <out-dir>")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveHostStoreCommand(file.Config.Runtime.Command)
	if err != nil {
		return err
	}
	name := args[1]
	outDir := args[2]

	// The managed rootfs is created at unit start under the systemd runtime dir
	// (%t): /run for system units, /run/user/<uid> for user units. The read-only
	// /nix/store cannot host the writable container root Podman needs, so we point
	// Rootfs there and prepare it via ExecStartPre using graft itself.
	graftBin, err := os.Executable()
	if err != nil {
		return err
	}
	runtimeRootfs := managedRootfsPath(name)
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	units, err := quadlet.RenderRootfsUnits(quadlet.RenderInput{
		Rootfs:                runtimeRootfs,
		FallbackContainerName: name,
		Command:               resolvedArgs,
		Config:                renderConfig,
		RootfsPrepare:         []string{graftBin, "prepare-rootfs", runtimeRootfs},
	})
	if err != nil {
		return err
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return err
	}
	for _, unit := range units {
		if err := os.WriteFile(filepath.Join(outDir, unit.Name), []byte(unit.Text), 0o644); err != nil {
			return err
		}
	}
	return nil
}

// managedRootfsPath is the writable rootfs location for a module-managed unit,
// expanded by systemd at runtime (%t = per-user or system runtime dir).
func managedRootfsPath(name string) string {
	return "%t/graft/" + name + "/rootfs"
}

func prepareRootfs(args []string) error {
	if len(args) != 1 {
		return errors.New("prepare-rootfs needs exactly one target directory")
	}
	return graftruntime.CreateMinimalRootfs(args[0])
}

// graftRun implements the dev path: reads a TOML blueprint, renders a transient
// Quadlet unit, and starts it as a detached user systemd service.
// --as <instance> is required and sets the instance/container name.
func graftRun(args []string) error {
	var tomlFile, instanceName string
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--as":
			if i+1 >= len(args) {
				return errors.New("--as requires an argument")
			}
			instanceName = args[i+1]
			i++
		default:
			if tomlFile == "" {
				tomlFile = args[i]
			} else {
				return fmt.Errorf("unexpected argument %q", args[i])
			}
		}
	}
	if tomlFile == "" {
		return errors.New("run needs: <file.toml> --as <instance>")
	}
	if instanceName == "" {
		return errors.New("run requires --as <instance> (worktree auto-naming is not yet implemented)")
	}
	file, err := config.LoadResolved(tomlFile, []string{configRoot()})
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	return startContainerAs(file, instanceName)
}

func runRootfs(args []string) error {
	cmdArgs := afterDash(args)
	if len(cmdArgs) == 0 {
		return errors.New("run-rootfs needs a command after --")
	}
	return runRootfsCommand(cmdArgs, config.Config{
		Filesystem: config.FilesystemConfig{
			Volumes: []config.VolumeConfig{{Source: "/nix/store", Target: "/nix/store", Mode: "ro"}},
		},
	})
}

func runRootfsCommand(cmdArgs []string, cfg config.Config) error {
	runtimeDir := graftruntime.RuntimeDir()
	runID := fmt.Sprintf("graft-%d-%d", os.Getpid(), time.Now().UnixNano())
	workDir := filepath.Join(runtimeDir, "graft", runID)
	rootfs := filepath.Join(workDir, "rootfs")

	if err := graftruntime.CreateMinimalRootfs(rootfs); err != nil {
		return err
	}
	defer func() { _ = os.RemoveAll(workDir) }()

	preparedCfg, review, promote, err := prepareTransientIsolation(cfg, workDir)
	if err != nil {
		return err
	}
	if err := resolveProxyDep(&preparedCfg); err != nil {
		return err
	}

	resolvedArgs, err := resolveRuntimeCommand(cmdArgs, preparedCfg.Runtime.Packages)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(preparedCfg, resolvedArgs)
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                rootfs,
		FallbackContainerName: runID,
		Command:               resolvedArgs,
		Config:                renderConfig,
	})
	if err != nil {
		return err
	}
	runErr := graftruntime.RunTransient(graftruntime.TransientInput{Quadlet: text, UnitStem: runID})
	reviewErr := review()
	if runErr != nil {
		return runErr
	}
	if reviewErr != nil {
		return reviewErr
	}
	return promote()
}

// nixBake reads package.json and package-lock.json from a directory, prefetches
// the npm deps hash using prefetch-npm-deps (from nixpkgs), and emits a
// buildNpmPackage Nix snippet ready to paste into a flake.
func nixBake(args []string) error {
	if len(args) != 1 {
		return errors.New("nix-bake needs: <dir>")
	}
	dir, err := filepath.Abs(args[0])
	if err != nil {
		return err
	}

	// Parse package.json
	pkgData, err := os.ReadFile(filepath.Join(dir, "package.json"))
	if err != nil {
		return fmt.Errorf("reading package.json: %w", err)
	}
	var pkg struct {
		Name    string `json:"name"`
		Version string `json:"version"`
		Bin     any    `json:"bin"`
		Main    string `json:"main"`
	}
	if err := json.Unmarshal(pkgData, &pkg); err != nil {
		return fmt.Errorf("parsing package.json: %w", err)
	}
	if pkg.Name == "" {
		pkg.Name = "my-agent"
	}
	if pkg.Version == "" {
		pkg.Version = "0.0.1"
	}

	// Require package-lock.json
	lockFile := filepath.Join(dir, "package-lock.json")
	if _, err := os.Stat(lockFile); err != nil {
		return fmt.Errorf("package-lock.json not found in %s; run \"npm install\" first", dir)
	}

	// Prefetch npm deps hash
	_, _ = fmt.Fprintf(os.Stderr, "graft: prefetching npm deps hash for %s@%s...\n", pkg.Name, pkg.Version)
	hash, err := prefetchNpmDeps(lockFile)
	if err != nil {
		return fmt.Errorf(
			"prefetch-npm-deps failed: %w\n\nRun manually and copy the hash:\n  nix run nixpkgs#prefetch-npm-deps -- %s",
			err, lockFile,
		)
	}

	nixName := nixIdentifier(pkg.Name)
	relDir := filepath.Base(dir)

	var b strings.Builder
	b.WriteString("# --- graft nix-bake output ---\n")
	b.WriteString("# Paste into your flake.nix or a dedicated .nix file.\n")
	b.WriteString("#\n")
	b.WriteString("# 1. Add the package to your flake packages:\n")
	b.WriteString("#      " + nixName + " = pkgs.callPackage ./" + relDir + ".nix { };\n")
	b.WriteString("#\n")
	b.WriteString("# 2. Reference it in your graft TOML:\n")
	b.WriteString("#      [config.runtime]\n")
	b.WriteString("#      packages = [" + fmt.Sprintf("%q", nixName) + "]\n")
	b.WriteString("#\n\n")
	b.WriteString("{ pkgs, ... }:\n\n")
	b.WriteString("pkgs.buildNpmPackage {\n")
	b.WriteString("  pname = " + fmt.Sprintf("%q", pkg.Name) + ";\n")
	b.WriteString("  version = " + fmt.Sprintf("%q", pkg.Version) + ";\n")
	b.WriteString("  src = ./" + relDir + ";\n")
	b.WriteString("  npmDepsHash = " + fmt.Sprintf("%q", hash) + ";\n")
	b.WriteString("\n")
	b.WriteString("  # If your package exposes a binary, add an installPhase:\n")
	b.WriteString("  # installPhase = ''\n")
	b.WriteString("  #   mkdir -p $out/bin $out/lib/node_modules/" + pkg.Name + "\n")
	b.WriteString("  #   cp -r . $out/lib/node_modules/" + pkg.Name + "\n")
	b.WriteString("  #   makeWrapper ${pkgs.nodejs}/bin/node $out/bin/" + nixName + " \\\n")
	b.WriteString("  #     --add-flags \"$out/lib/node_modules/" + pkg.Name + "/" + pkg.Main + "\"\n")
	b.WriteString("  # ''\n")
	b.WriteString("}\n")

	fmt.Print(b.String())
	return nil
}

// prefetchNpmDeps runs prefetch-npm-deps on the given package-lock.json and
// returns the trimmed hash string. It tries the tool from PATH first and falls
// back to nix run.
func prefetchNpmDeps(lockFile string) (string, error) {
	if path, err := exec.LookPath("prefetch-npm-deps"); err == nil {
		out, err := exec.Command(path, lockFile).Output()
		if err != nil {
			return "", err
		}
		return strings.TrimSpace(string(out)), nil
	}
	out, err := exec.Command("nix", "run", "--impure", "nixpkgs#prefetch-npm-deps", "--", lockFile).Output()
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

func startContainer(file *config.File) error {
	return startContainerAs(file, "")
}

// startContainerAs writes a Quadlet unit to the runtime Quadlet dir,
// daemon-reloads, and starts the systemd service. instanceName overrides the
// name from the config if non-empty (used by graft run --as).
func startContainerAs(file *config.File, instanceName string) error {
	name := instanceName
	if name == "" {
		name = file.Config.Container.Name
	}
	if name == "" {
		name = file.Name
	}
	if name == "" {
		return errors.New("config must have a name (or use --as to specify an instance name)")
	}

	cfg := file.Config
	if cfg.Container.Environment == nil {
		cfg.Container.Environment = map[string]string{}
	}

	// Handle home mode for detached containers.
	homeModeStr := cfg.Home.Mode
	if cfg.Home.Ephemeral && homeModeStr == "" {
		homeModeStr = "ephemeral"
	}
	homeTarget := cfg.Home.Target
	if homeTarget == "" {
		homeTarget = "/home/user"
	}
	switch homeModeStr {
	case "ephemeral":
		// For detached containers, "ephemeral" uses a stable per-name session dir
		// (not a truly ephemeral temp dir) so the container can be restarted.
		sessionDir := filepath.Join(userDataDir(), "graft", "sessions", name)
		if err := mountHomeDir(&cfg, sessionDir, homeTarget); err != nil {
			return err
		}
	case "persistent":
		if err := mountPersistentHome(&cfg, homeTarget); err != nil {
			return err
		}
	case "session":
		// Session mode: copy source into a stable per-name dir; write session
		// meta so `graft stop` / `graft down` can run review and promote.
		//
		// If a stale meta.json exists from a previous crash, process it first
		// so the user still gets a chance to review/promote the old session.
		if _, metaErr := os.Stat(sessionMetaPath(name)); metaErr == nil {
			_, _ = fmt.Fprintf(os.Stderr, "graft: stale session found for %q — running review/promote before restart\n", name)
			if err := handleSessionStop(name, ""); err != nil {
				return fmt.Errorf("stale session cleanup: %w", err)
			}
		}
		homeSrc, err := expandPath(cfg.Home.Source)
		if err != nil {
			return fmt.Errorf("home.source: %w", err)
		}
		if homeSrc == "" {
			return errors.New("home.source is required when home.mode = \"session\"")
		}
		sessionDir := filepath.Join(userDataDir(), "graft", "sessions", name, "home")
		if _, statErr := os.Stat(homeSrc); os.IsNotExist(statErr) {
			if err := os.MkdirAll(sessionDir, 0o755); err != nil {
				return err
			}
		} else {
			if err := copyTree(homeSrc, sessionDir, nil); err != nil {
				return fmt.Errorf("home session copy: %w", err)
			}
		}
		if err := mountHomeDir(&cfg, sessionDir, homeTarget); err != nil {
			return err
		}
		shadows, err := setupShadowMounts(name, &cfg)
		if err != nil {
			return err
		}
		if err := writeSessionMeta(name, sessionMeta{
			HomeSource:  homeSrc,
			HomeSession: sessionDir,
			HomeReview:  cfg.Home.Review,
			HomePromote: cfg.Home.Promote,
			Shadows:     shadows,
		}); err != nil {
			return fmt.Errorf("writing session meta: %w", err)
		}
	}

	// For non-session home modes that still define shadow mounts,
	// create per-session dirs and write a shadow-only meta file.
	if cfg.Home.Mode != "session" && len(cfg.Home.Shadow) > 0 {
		if _, metaErr := os.Stat(sessionMetaPath(name)); metaErr == nil {
			_, _ = fmt.Fprintf(os.Stderr, "graft: stale session found for %q — running review/promote before restart\n", name)
			if err := handleSessionStop(name, ""); err != nil {
				return fmt.Errorf("stale session cleanup: %w", err)
			}
		}
		shadows, err := setupShadowMounts(name, &cfg)
		if err != nil {
			return err
		}
		if err := writeSessionMeta(name, sessionMeta{Shadows: shadows}); err != nil {
			return fmt.Errorf("writing session meta: %w", err)
		}
	}

	if err := resolveProxyDep(&cfg); err != nil {
		return err
	}

	resolvedArgs, err := resolveRuntimeCommand(cfg.Runtime.Command, cfg.Runtime.Packages)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(cfg, resolvedArgs)

	graftBin, err := os.Executable()
	if err != nil {
		return err
	}
	runtimeRootfs := managedRootfsPath(name)

	// If this container IS a proxy server, embed its config into the rootfs
	// so "graft proxy serve" can find it at /run/graft-proxy.json.
	var extraStartPre [][]string
	if len(cfg.Proxy.Upstreams) > 0 {
		proxyJSON, marshalErr := json.Marshal(cfg.Proxy)
		if marshalErr != nil {
			return fmt.Errorf("serializing proxy config: %w", marshalErr)
		}
		proxyB64 := base64.StdEncoding.EncodeToString(proxyJSON)
		extraStartPre = [][]string{
			{graftBin, "write-proxy-config", runtimeRootfs, proxyB64},
		}
		if renderConfig.Container.Environment == nil {
			renderConfig.Container.Environment = map[string]string{}
		}
		renderConfig.Container.Environment[graftproxy.ConfigEnvVar] = "/run/graft-proxy.json"
	}

	units, err := quadlet.RenderRootfsUnits(quadlet.RenderInput{
		Rootfs:                runtimeRootfs,
		FallbackContainerName: name,
		Command:               resolvedArgs,
		Config:                renderConfig,
		RootfsPrepare:         []string{graftBin, "prepare-rootfs", runtimeRootfs},
		ExtraStartPre:         extraStartPre,
	})
	if err != nil {
		return err
	}

	runtimeDir := graftruntime.RuntimeDir()
	quadletDir := filepath.Join(runtimeDir, "containers", "systemd")
	if err := os.MkdirAll(quadletDir, 0o755); err != nil {
		return err
	}
	for _, unit := range units {
		if err := os.WriteFile(filepath.Join(quadletDir, unit.Name), []byte(unit.Text), 0o644); err != nil {
			return err
		}
	}
	if err := graftruntime.SystemctlUser("daemon-reload"); err != nil {
		return err
	}
	if err := graftruntime.SystemctlUser("start", name+".service"); err != nil {
		return err
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: started %s\n", name)
	_, _ = fmt.Fprintf(os.Stdout, "graft: attach:  graft attach %s\n", name)
	_, _ = fmt.Fprintf(os.Stdout, "graft: status:  graft list\n")
	_, _ = fmt.Fprintf(os.Stdout, "graft: stop:    graft stop %s\n", name)
	return nil
}

// graftStop stops a running container and removes its runtime Quadlet unit.
func graftStop(args []string, host string) error {
	if len(args) != 1 {
		return errors.New("stop needs: <container-name>")
	}
	name := args[0]
	if err := handleSessionStop(name, host); err != nil {
		return err
	}
	// Stop the systemd service; ignore error (container may already be stopped).
	_ = systemctlScope(host, "stop", name+".service")
	// Remove runtime unit files.
	runtimeDir := graftruntime.RuntimeDir()
	quadletDir := filepath.Join(runtimeDir, "containers", "systemd")
	for _, ext := range []string{".container", ".network", ".volume"} {
		_ = os.Remove(filepath.Join(quadletDir, name+ext))
	}
	_ = systemctlScope(host, "daemon-reload")
	_, _ = fmt.Fprintf(os.Stdout, "graft: stopped %s\n", name)
	return nil
}

func configRoot() string {
	if d := os.Getenv("GRAFT_CONFIG_ROOT"); d != "" {
		return d
	}
	return filepath.Dir(defaultConfigPath())
}

// graftStartOrAttach implements 'graft <instance>': start-or-attach in one step.
// The instance must already be deployed via nixos-rebuild / home-manager switch.
func graftStartOrAttach(name string, _ []string, host string) error {
	// Reject names with path separators to prevent confusion.
	if filepath.Base(name) != name || name == "." {
		return fmt.Errorf("invalid container name %q: must be a plain name without path separators", name)
	}
	// 1. Already running → attach immediately.
	running, err := isContainerRunning(name, host)
	if err != nil {
		return err
	}
	if running {
		return graftAttach([]string{name}, host)
	}
	// 2. Start the pre-deployed unit, then attach.
	if err := graftUp([]string{name}, host); err != nil {
		return err
	}
	ac := readAttachConfig(name, host)
	time.Sleep(ac.startDelay)
	return graftAttach([]string{name}, host)
}

// isContainerRunning returns true if a graft-managed container with the given
// name is currently running.
func isContainerRunning(name string, host string) (bool, error) {
	cmd := remoteExec(host, false, "podman", "ps",
		"--filter", "name=^"+name+"$",
		"--filter", "label=managed-by=graft",
		"--format", "{{.Names}}",
	)
	out, err := cmd.Output()
	if err != nil {
		return false, fmt.Errorf("podman ps: %w", err)
	}
	return strings.TrimSpace(string(out)) != "", nil
}

// shadowMeta records one shadow-mount entry written into the session meta file.
type shadowMeta struct {
	ContainerPath string `json:"containerPath"`
	HostPath      string `json:"hostPath"`
	SessionDir    string `json:"sessionDir"`
}

// sessionMeta records the home-session and shadow-mount configuration for a
// managed container so that `graft stop` / `graft down` can run review and
// promote on exit, and `graft diff` / `graft promote` can be used on demand.
type sessionMeta struct {
	HomeSource  string       `json:"homeSource"`
	HomeSession string       `json:"homeSession"`
	HomeReview  string       `json:"homeReview"`
	HomePromote string       `json:"homePromote"`
	Shadows     []shadowMeta `json:"shadows,omitempty"`
}

func sessionMetaPath(name string) string {
	return filepath.Join(userDataDir(), "graft", "sessions", name, "meta.json")
}

func writeSessionMeta(name string, meta sessionMeta) error {
	p := sessionMetaPath(name)
	if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
		return err
	}
	data, err := json.Marshal(meta)
	if err != nil {
		return err
	}
	return os.WriteFile(p, data, 0o600)
}

func readSessionMeta(name string) (sessionMeta, bool) {
	data, err := os.ReadFile(sessionMetaPath(name))
	if err != nil {
		return sessionMeta{}, false
	}
	var meta sessionMeta
	if err := json.Unmarshal(data, &meta); err != nil {
		return sessionMeta{}, false
	}
	return meta, true
}

// shadowDirName converts a container path to a safe directory name.
// e.g. "/workspace" → "workspace", "/home/user/data" → "home-user-data".
func shadowDirName(containerPath string) string {
	name := strings.TrimPrefix(containerPath, "/")
	name = strings.ReplaceAll(name, "/", "-")
	if name == "" {
		return "root"
	}
	return name
}

// setupShadowMounts creates per-session dirs for each [[home.shadow]] entry,
// seeds them from the host path, adds bind-mount entries to cfg, and returns
// the shadow metadata for writing into the session meta file.
func setupShadowMounts(name string, cfg *config.Config) ([]shadowMeta, error) {
	var shadows []shadowMeta
	for i, sm := range cfg.Home.Shadow {
		hostPath, err := expandPath(sm.Host)
		if err != nil {
			return nil, fmt.Errorf("home.shadow[%d].host: %w", i, err)
		}
		if sm.Container == "" {
			return nil, fmt.Errorf("home.shadow[%d].container is required", i)
		}
		sessionDir := filepath.Join(userDataDir(), "graft", "sessions", name, "shadow", shadowDirName(sm.Container))
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			return nil, err
		}
		if hostPath != "" {
			if _, statErr := os.Stat(hostPath); statErr == nil {
				if err := copyTree(hostPath, sessionDir, nil); err != nil {
					return nil, fmt.Errorf("shadow mount copy for %s: %w", sm.Container, err)
				}
			}
		}
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes, config.VolumeConfig{
			Source: sessionDir,
			Target: sm.Container,
			Mode:   "z",
		})
		shadows = append(shadows, shadowMeta{
			ContainerPath: sm.Container,
			HostPath:      hostPath,
			SessionDir:    sessionDir,
		})
	}
	return shadows, nil
}

// handleSessionStop runs the home-session review and promote steps for a
// managed container before it is stopped, then removes the session meta file.
// It is a no-op when no session meta exists for the given container name.
// Remote containers are skipped because the session dir lives on the remote.
func handleSessionStop(name, host string) error {
	if host != "" {
		return nil
	}
	meta, ok := readSessionMeta(name)
	if !ok {
		return nil
	}
	if meta.HomeReview == "diff" {
		if err := printWorkspaceDiff(meta.HomeSource, meta.HomeSession, nil); err != nil {
			return err
		}
	}
	switch meta.HomePromote {
	case "auto":
		if err := os.MkdirAll(meta.HomeSource, 0o755); err != nil {
			return err
		}
		_, _ = fmt.Fprintf(os.Stderr, "graft: applying home session changes to %s\n", meta.HomeSource)
		if err := applyWorkspace(meta.HomeSession, meta.HomeSource, nil); err != nil {
			return err
		}
	case "prompt":
		ok, err := promptUser("Apply home session changes to " + meta.HomeSource + "?")
		if err != nil {
			return err
		}
		if ok {
			if err := os.MkdirAll(meta.HomeSource, 0o755); err != nil {
				return err
			}
			if err := applyWorkspace(meta.HomeSession, meta.HomeSource, nil); err != nil {
				return err
			}
		}
	}
	_ = os.Remove(sessionMetaPath(name))
	return nil
}

// graftDiff shows a diff of shadow mounts and the home session for a named
// managed container. It is a no-op when no session meta exists.
// When --host is set the command is forwarded to the remote graft binary.
func graftDiff(args []string, host string) error {
	if len(args) == 0 {
		return errors.New("usage: graft diff <instance>")
	}
	if host != "" {
		cmd := remoteExec(host, false, "graft", append([]string{"diff"}, args...)...)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		return cmd.Run()
	}
	name := args[0]
	meta, ok := readSessionMeta(name)
	if !ok {
		return fmt.Errorf("no session data found for %q — has the container been started with home.mode = \"session\" or shadow mounts?", name)
	}
	any := false
	if meta.HomeSession != "" && meta.HomeSource != "" {
		_, _ = fmt.Fprintf(os.Stderr, "=== home session diff (%s) ===\n", meta.HomeSource)
		if err := printWorkspaceDiff(meta.HomeSource, meta.HomeSession, nil); err != nil {
			return err
		}
		any = true
	}
	for _, sm := range meta.Shadows {
		_, _ = fmt.Fprintf(os.Stderr, "=== shadow diff %s (%s) ===\n", sm.ContainerPath, sm.HostPath)
		if err := printWorkspaceDiff(sm.HostPath, sm.SessionDir, nil); err != nil {
			return err
		}
		any = true
	}
	if !any {
		_, _ = fmt.Fprintln(os.Stderr, "graft: nothing to diff")
	}
	return nil
}

// graftPromote copies shadow-mount session dirs back to their host paths.
// Use --path to promote a single shadow mount by its container path.
// When --host is set the command is forwarded to the remote graft binary.
func graftPromote(args []string, host string) error {
	if len(args) == 0 {
		return errors.New("usage: graft promote <instance> [--path <container-path>]")
	}
	if host != "" {
		// Use interactive=true so that a "prompt" promote can ask the user.
		cmd := remoteExec(host, true, "graft", append([]string{"promote"}, args...)...)
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		return cmd.Run()
	}
	name := args[0]
	filterPath, _ := extractFlag(args[1:], "--path")
	meta, ok := readSessionMeta(name)
	if !ok {
		return fmt.Errorf("no session data found for %q", name)
	}
	promoted := 0
	for _, sm := range meta.Shadows {
		if filterPath != "" && sm.ContainerPath != filterPath {
			continue
		}
		if sm.HostPath == "" {
			_, _ = fmt.Fprintf(os.Stderr, "graft: shadow %s has no host path — skipping\n", sm.ContainerPath)
			continue
		}
		_, _ = fmt.Fprintf(os.Stderr, "graft: promoting %s → %s\n", sm.ContainerPath, sm.HostPath)
		if err := os.MkdirAll(sm.HostPath, 0o755); err != nil {
			return err
		}
		if err := applyWorkspace(sm.SessionDir, sm.HostPath, nil); err != nil {
			return err
		}
		promoted++
	}
	if promoted == 0 {
		_, _ = fmt.Fprintln(os.Stderr, "graft: nothing to promote")
	}
	return nil
}

// graftReset removes all session data for a named container (home session and
// shadow mounts). The next `graft up` starts with a clean slate.
// When --host is set the command is forwarded to the remote graft binary.
func graftReset(args []string, host string) error {
	if len(args) == 0 {
		return errors.New("usage: graft reset <instance>")
	}
	if host != "" {
		cmd := remoteExec(host, false, "graft", append([]string{"reset"}, args...)...)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		return cmd.Run()
	}
	name := args[0]
	sessionBase := filepath.Join(userDataDir(), "graft", "sessions", name)
	if _, err := os.Stat(sessionBase); os.IsNotExist(err) {
		_, _ = fmt.Fprintf(os.Stderr, "graft: no session data for %q\n", name)
		return nil
	}
	if err := os.RemoveAll(sessionBase); err != nil {
		return fmt.Errorf("removing session data: %w", err)
	}
	_, _ = fmt.Fprintf(os.Stderr, "graft: session data for %q removed\n", name)
	return nil
}

// userDataDir returns XDG_DATA_HOME or ~/.local/share.
func userDataDir() string {
	if d := os.Getenv("XDG_DATA_HOME"); d != "" {
		return d
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "/tmp"
	}
	return filepath.Join(home, ".local", "share")
}

// nixIdentifier converts an npm package name to a valid Nix identifier.
// Examples: @anthropic-ai/sdk → anthropic-ai-sdk, my.package → my-package
func nixIdentifier(name string) string {
	name = strings.TrimPrefix(name, "@")
	name = strings.ReplaceAll(name, "/", "-")
	var b strings.Builder
	for _, r := range name {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '-', r == '_':
			b.WriteRune(r)
		default:
			b.WriteRune('-')
		}
	}
	return b.String()
}

func withRuntimePathEnv(cfg config.Config, args []string) config.Config {
	if len(args) == 0 || !strings.HasPrefix(args[0], "/nix/store/") {
		return cfg
	}
	out := cfg
	if out.Container.Environment == nil {
		out.Container.Environment = map[string]string{}
	}
	if _, ok := out.Container.Environment["PATH"]; !ok {
		out.Container.Environment["PATH"] = filepath.Dir(args[0])
	}
	return out
}

func rejectUnresolvedGraphFeatures(file *config.File) error {
	if len(file.Parents.Add) > 0 || len(file.Parents.Set) > 0 || len(file.Parents.Remove) > 0 || len(file.Children.Add) > 0 || len(file.Children.Set) > 0 || len(file.Children.Remove) > 0 {
		return errors.New("direct CLI run/render does not resolve parents/children yet; use NixOS/Home Manager configRoot or an effective TOML")
	}
	ops := file.Config.Runtime.PackageOps
	if len(ops.Add) > 0 || len(ops.Remove) > 0 || len(ops.Replace) > 0 {
		return errors.New("direct CLI run/render does not apply config.runtime.packageOps yet; use NixOS/Home Manager configRoot or an effective TOML")
	}
	return nil
}

func validateRunnable(runtime config.RuntimeConfig) error {
	if runtime.Mode != "rootfs-store" {
		return fmt.Errorf("config.runtime.mode must be rootfs-store for runnable configs, got %q", runtime.Mode)
	}
	if len(runtime.Command) == 0 {
		return errors.New("config.runtime.command must not be empty for runnable configs")
	}
	return nil
}

func resolveHostStoreCommand(args []string) ([]string, error) {
	if len(args) == 0 {
		return nil, errors.New("command is required")
	}
	if filepath.IsAbs(args[0]) {
		return args, nil
	}
	return resolveFromHostPath(args)
}

func resolveRuntimeCommand(args []string, packageNames []string) ([]string, error) {
	if len(args) == 0 {
		return nil, errors.New("command is required")
	}
	if filepath.IsAbs(args[0]) {
		return args, nil
	}
	if len(packageNames) > 0 {
		runtimeEnv, err := buildRuntimeEnv(packageNames)
		if err != nil {
			return nil, err
		}
		candidate := filepath.Join(runtimeEnv, "bin", args[0])
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			return append([]string{candidate}, args[1:]...), nil
		}
		return nil, fmt.Errorf("command %q not found in runtime packages at %s/bin", args[0], runtimeEnv)
	}
	return resolveFromHostPath(args)
}

// resolveFromHostPath resolves a non-absolute command via the host PATH and
// requires it to live inside /nix/store, so runs never depend on host-installed
// binaries outside the Nix store.
func resolveFromHostPath(args []string) ([]string, error) {
	path, err := exec.LookPath(args[0])
	if err != nil {
		return nil, fmt.Errorf("command %q not found on host PATH; use an absolute /nix/store path or set config.runtime.packages", args[0])
	}
	if !strings.HasPrefix(path, "/nix/store/") {
		return nil, fmt.Errorf("command %q resolved to %s, which is not inside /nix/store; set config.runtime.packages or use an absolute /nix/store path", args[0], path)
	}
	return append([]string{path}, args[1:]...), nil
}

var packageNamePattern = regexp.MustCompile(`^[A-Za-z0-9._+-]+$`)

// nixSystem returns the Nix system string for the current host (e.g.
// "x86_64-linux") derived from Go's compile-time GOOS/GOARCH constants.
// No subprocess is needed, so this is always pure.
func nixSystem() (string, error) {
	var arch string
	switch runtime.GOARCH {
	case "amd64":
		arch = "x86_64"
	case "arm64":
		arch = "aarch64"
	default:
		return "", fmt.Errorf("unsupported architecture %q for Nix package resolution", runtime.GOARCH)
	}
	switch runtime.GOOS {
	case "linux", "darwin":
		// supported
	default:
		return "", fmt.Errorf("unsupported OS %q for Nix package resolution", runtime.GOOS)
	}
	return arch + "-" + runtime.GOOS, nil
}

func buildRuntimeEnv(packageNames []string) (string, error) {
	for _, packageName := range packageNames {
		if !packageNamePattern.MatchString(packageName) {
			return "", fmt.Errorf("invalid package name %q; expected only letters, numbers, dot, underscore, plus, or dash", packageName)
		}
	}
	if nixpkgsStorePath == "" {
		return "", errors.New(
			"graft was not built with a pinned nixpkgs store path\n" +
				"  use: nix run github:zerodawn1990/graft or nix build github:zerodawn1990/graft\n" +
				"  bare binary builds cannot guarantee supply-chain integrity",
		)
	}
	system, err := nixSystem()
	if err != nil {
		return "", err
	}
	expr := `let
  pkgs = import ` + nixpkgsStorePath + ` { system = ` + nixString(system) + `; };
  packageNames = [` + nixStringList(packageNames) + ` ];
  packages = map (name:
    if builtins.hasAttr name pkgs then builtins.getAttr name pkgs
    else throw "unknown package ${name}"
  ) packageNames;
in pkgs.buildEnv { name = "graft-runtime"; paths = packages; }`
	cmd := exec.Command("nix", "build", "--no-link", "--print-out-paths", "--expr", expr)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("failed to build runtime closure with nix: %w\n%s", err, strings.TrimSpace(string(out)))
	}
	path := strings.TrimSpace(string(out))
	if path == "" {
		return "", errors.New("nix build returned an empty runtime path")
	}
	if fields := strings.Fields(path); len(fields) > 0 {
		path = fields[len(fields)-1]
	}
	return path, nil
}

func nixStringList(values []string) string {
	quoted := make([]string, len(values))
	for i, value := range values {
		quoted[i] = nixString(value)
	}
	return strings.Join(quoted, " ")
}

func nixString(value string) string {
	value = strings.ReplaceAll(value, `\`, `\\`)
	value = strings.ReplaceAll(value, `"`, `\"`)
	value = strings.ReplaceAll(value, `${`, `\${`)
	return `"` + value + `"`
}
