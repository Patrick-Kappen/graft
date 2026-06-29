package cli

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/zerodawn1990/podman-agent-container/internal/config"
	"github.com/zerodawn1990/podman-agent-container/internal/quadlet"
	pacruntime "github.com/zerodawn1990/podman-agent-container/internal/runtime"
)

const Version = "0.1.0"

func Main(args []string) int {
	if err := Run(args); err != nil {
		fmt.Fprintf(os.Stderr, "podman-agent-container: %v\n", err)
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
		return up(args[1:])
	case "render":
		return renderYAML(args[1:])
	case "render-nixos":
		return renderNixOS(args[1:])
	case "run":
		return runYAML(args[1:])
	case "run-rootfs":
		return runRootfs(args[1:])
	default:
		return fmt.Errorf("unknown command %q", args[0])
	}
}

func Usage(w io.Writer) {
	_, _ = fmt.Fprint(w, `podman-agent-container

Usage:
  podman-agent-container config path
  podman-agent-container config init [path]
  podman-agent-container config show [path]
  pac up [file.toml]
  pac inspect <file.toml>
  pac render <file.toml>
  pac render-nixos <file.toml> <rootfs> <container-name>
  pac run <file.toml>
  pac run-rootfs -- <command> [args...]

Commands:
  config       Manage the no-op example config
  up           Run a TOML config, autodetecting one if omitted
  inspect      Inspect a TOML config and print JSON metadata
  render       Render a minimal TOML config to Quadlet text
  render-nixos Render a minimal TOML config with concrete NixOS store paths
  run          Run a minimal TOML config through a temporary Quadlet unit
  run-rootfs   Run a command through a temporary rootfs Quadlet unit
`)
}

func runConfig(args []string) error {
	if len(args) == 0 {
		return errors.New("config needs a subcommand: path, init, show")
	}

	switch args[0] {
	case "path":
		fmt.Println(defaultConfigPath())
		return nil
	case "init":
		path := defaultConfigPath()
		if len(args) > 1 {
			path = args[1]
		}
		return initConfig(path)
	case "show":
		path := defaultConfigPath()
		if len(args) > 1 {
			path = args[1]
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		fmt.Print(string(data))
		return nil
	default:
		return fmt.Errorf("unknown config subcommand %q", args[0])
	}
}

func defaultConfigPath() string {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, "podman-agent-container", "config.yaml")
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		return "config.yaml"
	}
	return filepath.Join(home, ".config", "podman-agent-container", "config.yaml")
}

func initConfig(path string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	if _, err := os.Stat(path); err == nil {
		return fmt.Errorf("config already exists: %s", path)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return os.WriteFile(path, []byte(exampleConfig), 0o644)
}

const exampleConfig = `# podman-agent-container config
#
# Empty means no-op. This template intentionally does not define containers,
# presets, mounts, runtimes, or security policy.

version = 1
name = "example"

[parents]
add = []
remove = []
set = []

[children]
add = []
remove = []
set = []

[config]
# Empty means no-op.
`

type inspectOutput struct {
	Version     int    `json:"version"`
	Name        string `json:"name,omitempty"`
	Noop        bool   `json:"noop"`
	RuntimeMode string `json:"runtimeMode,omitempty"`
}

func inspectYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("inspect needs exactly one TOML file")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	output := inspectOutput{
		Version:     file.Version,
		Name:        file.Name,
		Noop:        file.IsNoop(),
		RuntimeMode: file.Config.Runtime.Mode,
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	return encoder.Encode(output)
}

func up(args []string) error {
	if len(args) > 1 {
		return errors.New("up accepts at most one TOML file")
	}
	path := ""
	if len(args) == 1 {
		path = args[0]
	} else {
		var err error
		path, err = autodetectConfig()
		if err != nil {
			return err
		}
	}
	return runYAML([]string{path})
}

func autodetectConfig() (string, error) {
	candidates := []string{
		"pac.toml",
		"podman-agent-container.toml",
		".pac.toml",
		"config.toml",
	}
	for _, candidate := range candidates {
		info, err := os.Stat(candidate)
		if err == nil && !info.IsDir() {
			return candidate, nil
		}
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
	}
	return "", errors.New("no TOML config found; tried pac.toml, podman-agent-container.toml, .pac.toml, config.toml")
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
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveRuntimeCommand(file.Config.Runtime.Command, file.Config.Runtime.Packages)
	if err != nil {
		return err
	}
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                "<runtime-rootfs>",
		FallbackContainerName: "<runtime-container-name>",
		Command:               resolvedArgs,
		Config:                file.Config,
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
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                args[1],
		FallbackContainerName: args[2],
		Command:               resolvedArgs,
		Config:                file.Config,
	})
	if err != nil {
		return err
	}
	fmt.Print(text)
	return nil
}

func runYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("run needs exactly one TOML file")
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
	return runRootfsCommand(file.Config.Runtime.Command, file.Config)
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
	runtimeDir := pacruntime.RuntimeDir()
	runID := fmt.Sprintf("pac-%d-%d", os.Getpid(), time.Now().UnixNano())
	workDir := filepath.Join(runtimeDir, "podman-agent-container", runID)
	rootfs := filepath.Join(workDir, "rootfs")

	if err := pacruntime.CreateMinimalRootfs(rootfs); err != nil {
		return err
	}
	defer func() { _ = os.RemoveAll(workDir) }()

	resolvedArgs, err := resolveRuntimeCommand(cmdArgs, cfg.Runtime.Packages)
	if err != nil {
		return err
	}
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                rootfs,
		FallbackContainerName: runID,
		Command:               resolvedArgs,
		Config:                cfg,
	})
	if err != nil {
		return err
	}
	return pacruntime.RunTransient(pacruntime.TransientInput{Quadlet: text, UnitStem: runID})
}

func afterDash(args []string) []string {
	if len(args) > 0 && args[0] == "--" {
		return args[1:]
	}
	return args
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
	path, err := exec.LookPath(args[0])
	if err != nil {
		return nil, fmt.Errorf("command %q not found on host PATH; use an absolute /nix/store path or set config.runtime.packages", args[0])
	}
	if !strings.HasPrefix(path, "/nix/store/") {
		return nil, fmt.Errorf("command %q resolved to %s, which is not inside /nix/store", args[0], path)
	}
	return append([]string{path}, args[1:]...), nil
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
	path, err := exec.LookPath(args[0])
	if err != nil {
		return nil, fmt.Errorf("command %q not found on host PATH; use an absolute /nix/store path or set config.runtime.packages", args[0])
	}
	if !strings.HasPrefix(path, "/nix/store/") {
		return nil, fmt.Errorf("command %q resolved to %s, which is not inside /nix/store; set config.runtime.packages or use an absolute /nix/store path", args[0], path)
	}
	return append([]string{path}, args[1:]...), nil
}

func buildRuntimeEnv(packageNames []string) (string, error) {
	expr := `let
  pkgs = import (builtins.getFlake "nixpkgs") { system = builtins.currentSystem; };
  packageNames = [` + nixStringList(packageNames) + ` ];
  packages = map (name:
    if builtins.hasAttr name pkgs then builtins.getAttr name pkgs
    else throw "unknown package ${name}"
  ) packageNames;
in pkgs.buildEnv { name = "pac-runtime"; paths = packages; }`
	cmd := exec.Command("nix", "build", "--no-link", "--print-out-paths", "--impure", "--expr", expr)
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
	value = strings.ReplaceAll(value, `\\`, `\\\\`)
	value = strings.ReplaceAll(value, `"`, `\\"`)
	value = strings.ReplaceAll(value, `${`, `\${`)
	return `"` + value + `"`
}
