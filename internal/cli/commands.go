package cli

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
	"github.com/zerodawn1990/graft/internal/config"
	"github.com/zerodawn1990/graft/internal/quadlet"
	graftruntime "github.com/zerodawn1990/graft/internal/runtime"
)

const Version = "0.1.0"

// flagSlice is a flag.Value that accumulates repeated flag values.
type flagSlice []string

func (f *flagSlice) String() string { return strings.Join(*f, ", ") }
func (f *flagSlice) Set(v string) error { *f = append(*f, v); return nil }

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
		return up(args[1:])
	case "render":
		return renderYAML(args[1:])
	case "render-nixos":
		return renderNixOS(args[1:])
	case "render-nixos-units":
		return renderNixOSUnits(args[1:])
	case "prepare-rootfs":
		return prepareRootfs(args[1:])
	case "run":
		return runYAML(args[1:])
	case "run-rootfs":
		return runRootfs(args[1:])
	case "nix-bake":
		return nixBake(args[1:])
	case "agent":
		return graftAgent(args[1:])
	case "start":
		return graftStart(args[1:])
	case "attach":
		return graftAttach(args[1:])
	case "list":
		return graftList(args[1:])
	case "stop":
		return graftStop(args[1:])
	default:
		// Try as a named agent shortcut (saved with 'graft agent save').
		return graftNamedAgent(args[0], args[1:])
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
  graft stop <container-name>
  graft agent start <name> [flags]
  graft agent save  <name> [flags]
  graft agent attach <name>
  graft agent list
  graft agent stop <name>
  graft <name>                         (start-or-attach a saved agent)

Commands:
  config       Manage the no-op example config
  up           Run a TOML config, autodetecting one if omitted
  inspect      Inspect a TOML config and print JSON metadata
  render       Render a minimal TOML config to Quadlet text
  render-nixos Render a minimal TOML config with concrete NixOS store paths
  render-nixos-units Render all Quadlet units with concrete NixOS store paths
  prepare-rootfs Create a writable minimal rootfs (used by managed units at start)
  run          Run a minimal TOML config through a temporary Quadlet unit
  run-rootfs   Run a command through a temporary rootfs Quadlet unit
  nix-bake     Read package.json + package-lock.json, prefetch npm hash, emit buildNpmPackage Nix snippet
  start        Start a container detached (writes Quadlet unit, daemon-reload, systemctl start)
  attach       Attach to a running container's tmux session (podman exec -it <name> tmux attach)
  agent        High-level agent management without a TOML file
               graft agent start <name> -p <pkg> [--project <dir>] [-f host:container[:mode]] [-e KEY=VAL]
               graft agent save  <name> [same flags] — save for later use with 'graft <name>'
  <name>       Start (if needed) and attach to a saved agent (shorthand for agent start-or-attach)
  list         List running graft-managed containers
  stop         Stop a running container and remove its runtime unit
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
		return filepath.Join(xdg, "graft", "config.toml")
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		return "config.toml"
	}
	return filepath.Join(home, ".config", "graft", "config.toml")
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

const exampleConfig = `# graft config
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
		"graft.toml",
		".graft.toml",
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
	return "", errors.New("no TOML config found; tried graft.toml, .graft.toml, config.toml")
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

func runYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("run needs exactly one TOML file")
	}
	file, err := config.LoadResolved(args[0], []string{configRoot()})
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

func prepareTransientIsolation(cfg config.Config, workDir string) (config.Config, func() error, func() error, error) {
	review := func() error { return nil }
	promote := func() error { return nil }
	prepared := cfg

	if prepared.Container.Environment == nil {
		prepared.Container.Environment = map[string]string{}
	}

	// Resolve home mode: legacy Ephemeral field is equivalent to mode="ephemeral".
	homeMode := prepared.Home.Mode
	if prepared.Home.Ephemeral && homeMode == "" {
		homeMode = "ephemeral"
	}
	homeTarget := prepared.Home.Target
	if homeTarget == "" {
		homeTarget = "/home/agent"
	}
	setHomeEnv := func() {
		prepared.Container.Environment["HOME"] = homeTarget
		prepared.Container.Environment["XDG_CONFIG_HOME"] = filepath.Join(homeTarget, ".config")
		prepared.Container.Environment["XDG_CACHE_HOME"] = filepath.Join(homeTarget, ".cache")
		prepared.Container.Environment["XDG_DATA_HOME"] = filepath.Join(homeTarget, ".local", "share")
		prepared.Container.Environment["XDG_STATE_HOME"] = filepath.Join(homeTarget, ".local", "state")
	}
	switch homeMode {
	case "ephemeral":
		homeDir := filepath.Join(workDir, "home")
		if err := os.MkdirAll(homeDir, 0o755); err != nil {
			return config.Config{}, nil, nil, err
		}
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes, config.VolumeConfig{Source: homeDir, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
	case "persistent":
		// Persistent home survives across runs; sessions, auth tokens, etc. are stored here.
		homeSrc, err := expandPath(prepared.Home.Source)
		if err != nil {
			return config.Config{}, nil, nil, fmt.Errorf("home.source: %w", err)
		}
		if homeSrc == "" {
			return config.Config{}, nil, nil, errors.New("home.source is required when home.mode = \"persistent\"")
		}
		if err := os.MkdirAll(homeSrc, 0o755); err != nil {
			return config.Config{}, nil, nil, err
		}
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes, config.VolumeConfig{Source: homeSrc, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
	}

	if prepared.Workspace.Mode == "" || prepared.Workspace.Mode == "none" {
		return prepared, review, promote, nil
	}
	if prepared.Workspace.Mode != "copy" {
		return config.Config{}, nil, nil, fmt.Errorf("unsupported workspace mode %q", prepared.Workspace.Mode)
	}

	skipDirs := workspaceSkipDirs
	if len(prepared.Workspace.ExcludePatterns) > 0 {
		skipDirs = prepared.Workspace.ExcludePatterns
	}

	wsSource := prepared.Workspace.Source
	if wsSource == "" {
		wsSource = "."
	}
	absSource, err := filepath.Abs(wsSource)
	if err != nil {
		return config.Config{}, nil, nil, err
	}
	wsTarget := prepared.Workspace.Target
	if wsTarget == "" {
		wsTarget = "/workspace"
	}
	dest := filepath.Join(workDir, "workspace")
	if err := copyTree(absSource, dest, skipDirs); err != nil {
		return config.Config{}, nil, nil, err
	}
	prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes, config.VolumeConfig{Source: dest, Target: wsTarget, Mode: "rw"})
	if prepared.Container.WorkingDir == "" {
		prepared.Container.WorkingDir = wsTarget
	}
	if prepared.Workspace.Review == "diff" {
		review = func() error { return printWorkspaceDiff(absSource, dest, skipDirs) }
	}
	switch prepared.Workspace.Promote {
	case "auto":
		promote = func() error {
			_, _ = fmt.Fprintf(os.Stderr, "graft: applying workspace changes to %s\n", absSource)
			return applyWorkspace(dest, absSource, skipDirs)
		}
	case "prompt":
		promote = func() error {
			ok, err := promptUser("Apply workspace changes to " + absSource + "?")
			if err != nil || !ok {
				return err
			}
			return applyWorkspace(dest, absSource, skipDirs)
		}
	}
	return prepared, review, promote, nil
}

func copyTree(source, dest string, skipDirs []string) error {
	return filepath.WalkDir(source, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if path == source {
			return os.MkdirAll(dest, 0o755)
		}
		name := entry.Name()
		if entry.IsDir() && shouldSkipDir(name, skipDirs) {
			return filepath.SkipDir
		}
		rel, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		outPath := filepath.Join(dest, rel)
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return os.MkdirAll(outPath, info.Mode().Perm())
		}
		if info.Mode()&os.ModeType != 0 {
			return nil
		}
		return copyFile(path, outPath, info.Mode().Perm())
	})
}

var workspaceSkipDirs = []string{".git", ".jj", ".go", ".direnv", "result", "node_modules"}

func shouldSkipDir(name string, skipDirs []string) bool {
	for _, dir := range skipDirs {
		if name == dir {
			return true
		}
	}
	return false
}

func copyFile(source, dest string, mode fs.FileMode) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer func() { _ = in.Close() }()
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return err
	}
	out, err := os.OpenFile(dest, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, mode)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	closeErr := out.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
}

func printWorkspaceDiff(source, candidate string, skipDirs []string) error {
	diffArgs := []string{"-ruN"}
	for _, dir := range skipDirs {
		diffArgs = append(diffArgs, "--exclude="+dir)
	}
	diffArgs = append(diffArgs, source, candidate)
	cmd := exec.Command("diff", diffArgs...)
	out, err := cmd.CombinedOutput()
	if len(out) > 0 {
		_, _ = fmt.Fprintln(os.Stdout, "--- graft workspace diff ---")
		fmt.Print(string(out))
	}
	if err == nil {
		_, _ = fmt.Fprintln(os.Stdout, "--- graft workspace diff: no changes ---")
		return nil
	}
	if exitErr, ok := err.(*exec.ExitError); ok && exitErr.ExitCode() == 1 {
		return nil
	}
	return err
}

func afterDash(args []string) []string {
	if len(args) > 0 && args[0] == "--" {
		return args[1:]
	}
	return args
}

// applyWorkspace copies all files from the workspace candidate (what ran inside
// the container) back to the real source directory. New and modified files are
// applied; files deleted inside the container are left untouched on the host.
func applyWorkspace(candidate, dest string, skipDirs []string) error {
	return copyTree(candidate, dest, skipDirs)
}

// promptUser prints a question and reads a y/yes/n/N answer from stdin.
func promptUser(question string) (bool, error) {
	_, _ = fmt.Fprintf(os.Stdout, "%s [y/N] ", question)
	var response string
	if _, err := fmt.Fscan(os.Stdin, &response); err != nil {
		return false, nil
	}
	r := strings.ToLower(strings.TrimSpace(response))
	return r == "y" || r == "yes", nil
}

// expandPath expands a leading ~ to the user's home directory and expands
// environment variables in p.
func expandPath(p string) (string, error) {
	if p == "" {
		return "", nil
	}
	if strings.HasPrefix(p, "~/") {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		return filepath.Join(home, p[2:]), nil
	}
	return os.ExpandEnv(p), nil
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

// graftStart starts a container detached: resolves the config, writes a Quadlet unit to the
// runtime Quadlet dir, daemon-reloads, and starts the systemd service. Returns immediately.
func graftStart(args []string) error {
	if len(args) != 1 {
		return errors.New("start needs: <file.toml>")
	}
	file, err := config.LoadResolved(args[0], []string{configRoot()})
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	return startContainer(file)
}

func startContainer(file *config.File) error {
	name := file.Config.Container.Name
	if name == "" {
		name = file.Name
	}
	if name == "" {
		return errors.New("config must have a name")
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
		homeTarget = "/home/agent"
	}
	setStartHomeEnv := func() {
		cfg.Container.Environment["HOME"] = homeTarget
		cfg.Container.Environment["XDG_CONFIG_HOME"] = filepath.Join(homeTarget, ".config")
		cfg.Container.Environment["XDG_CACHE_HOME"] = filepath.Join(homeTarget, ".cache")
		cfg.Container.Environment["XDG_DATA_HOME"] = filepath.Join(homeTarget, ".local", "share")
		cfg.Container.Environment["XDG_STATE_HOME"] = filepath.Join(homeTarget, ".local", "state")
	}
	switch homeModeStr {
	case "ephemeral":
		// For detached containers, "ephemeral" uses a stable per-name session dir
		// (not a truly ephemeral temp dir) so the container can be restarted.
		sessionDir := filepath.Join(userDataDir(), "graft", "sessions", name)
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			return err
		}
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes, config.VolumeConfig{Source: sessionDir, Target: homeTarget, Mode: "rw"})
		setStartHomeEnv()
	case "persistent":
		homeSrc, err := expandPath(cfg.Home.Source)
		if err != nil {
			return fmt.Errorf("home.source: %w", err)
		}
		if homeSrc == "" {
			return errors.New("home.source is required when home.mode = \"persistent\"")
		}
		if err := os.MkdirAll(homeSrc, 0o755); err != nil {
			return err
		}
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes, config.VolumeConfig{Source: homeSrc, Target: homeTarget, Mode: "rw"})
		setStartHomeEnv()
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

// graftAttach attaches to a running container's tmux session. If no session named
// "main" exists, it starts a new interactive tmux session inside the container.
func graftAttach(args []string) error {
	if len(args) < 1 {
		return errors.New("attach needs: <container-name>")
	}
	name := args[0]
	attach := exec.Command("podman", "exec", "-it", name, "tmux", "attach-session", "-t", "main")
	attach.Stdin, attach.Stdout, attach.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := attach.Run(); err == nil {
		return nil
	}
	// No existing session — start a new one.
	_, _ = fmt.Fprintf(os.Stderr, "graft: no tmux session 'main' found, starting new session\n")
	newSession := exec.Command("podman", "exec", "-it", name, "tmux", "new-session", "-s", "main")
	newSession.Stdin, newSession.Stdout, newSession.Stderr = os.Stdin, os.Stdout, os.Stderr
	return newSession.Run()
}

// graftList lists running containers that carry the managed-by=graft label.
func graftList(_ []string) error {
	cmd := exec.Command("podman", "ps",
		"--filter", "label=managed-by=graft",
		"--format", `table {{.Names}}\t{{.Status}}\t{{.RunningFor}}`)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}

// graftStop stops a running container and removes its runtime Quadlet unit.
func graftStop(args []string) error {
	if len(args) != 1 {
		return errors.New("stop needs: <container-name>")
	}
	name := args[0]
	// Stop the systemd service; ignore error (container may already be stopped).
	_ = graftruntime.SystemctlUser("stop", name+".service")
	// Remove runtime unit files.
	runtimeDir := graftruntime.RuntimeDir()
	quadletDir := filepath.Join(runtimeDir, "containers", "systemd")
	for _, ext := range []string{".container", ".network", ".volume"} {
		_ = os.Remove(filepath.Join(quadletDir, name+ext))
	}
	_ = graftruntime.SystemctlUser("daemon-reload")
	_, _ = fmt.Fprintf(os.Stdout, "graft: stopped %s\n", name)
	return nil
}

// graftAgent dispatches 'graft agent <sub>' commands.
func graftAgent(args []string) error {
	if len(args) < 1 {
		return errors.New("agent needs a subcommand: start, stop, list, attach")
	}
	switch args[0] {
	case "start":
		return graftAgentStart(args[1:])
	case "save":
		return graftAgentSave(args[1:])
	case "stop":
		return graftStop(args[1:])
	case "list":
		return graftList(args[1:])
	case "attach":
		return graftAttach(args[1:])
	default:
		return fmt.Errorf("unknown agent subcommand %q; expected start, save, stop, list, attach", args[0])
	}
}

// graftAgentStart starts a detached tmux agent without requiring a TOML file.
//
//	graft agent start <name> \
//	  -p pi-agent \
//	  --project ~/projects/my-project \
//	  -f ~/system-prompt.md:/etc/agent/system-prompt.md:ro \
//	  -e MY_API_KEY=sk-…
func graftAgentStart(args []string) error {
	fs := flag.NewFlagSet("agent start", flag.ContinueOnError)
	var packages flagSlice
	var files flagSlice
	var envVars flagSlice
	var project, homePath, cmdOverride string
	var ephemeral bool

	fs.Var(&packages, "package", "nix `package` to include in PATH (repeatable)")
	fs.Var(&packages, "p", "nix package (shorthand for --package)")
	fs.Var(&files, "file", "`host:container[:mode]` bind-mount (repeatable); mode defaults to ro")
	fs.Var(&files, "f", "bind-mount (shorthand for --file)")
	fs.Var(&envVars, "env", "`KEY=VALUE` environment variable (repeatable)")
	fs.Var(&envVars, "e", "environment variable (shorthand for --env)")
	fs.StringVar(&project, "project", "", "project `dir` to mount at /workspace (host-path[:container-path])")
	fs.StringVar(&homePath, "home", "", "persistent home `path` (default: ~/.local/share/graft/sessions/<name>)")
	fs.StringVar(&cmdOverride, "cmd", "", "override agent entry-point `binary` (default: first --package name)")
	fs.BoolVar(&ephemeral, "ephemeral", false, "use ephemeral home (wiped on stop) instead of persistent")
	fs.Usage = func() {
		fmt.Fprint(os.Stderr, "Usage: graft agent start <name> [flags]\n\nFlags:\n")
		fs.PrintDefaults()
	}

	// Accept <name> as the first arg even when flags follow it.
	// Go's flag stops at the first non-flag arg, so we pre-extract the name.
	var name string
	flagArgs := args
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		name = args[0]
		flagArgs = args[1:]
	}

	if err := fs.Parse(flagArgs); err != nil {
		return err
	}
	// Also support name as the only trailing positional arg.
	if name == "" {
		switch len(fs.Args()) {
		case 1:
			name = fs.Args()[0]
		case 0:
			fs.Usage()
			return errors.New("agent start needs exactly one <name>")
		default:
			fs.Usage()
			return errors.New("agent start: unexpected extra arguments")
		}
	} else if len(fs.Args()) != 0 {
		fs.Usage()
		return errors.New("agent start: unexpected extra arguments after name")
	}

	// Resolve entry-point: tmux wraps the agent binary.
	agentBin := cmdOverride
	if agentBin == "" {
		if len(packages) == 0 {
			return errors.New("--package/-p or --cmd is required")
		}
		agentBin = filepath.Base(string(packages[0]))
	}
	command := []string{"tmux", "new-session", "-A", "-s", "main", agentBin}

	// Packages: tmux is always included.
	pkgs := []string{"tmux"}
	for _, p := range packages {
		if p != "tmux" {
			pkgs = append(pkgs, p)
		}
	}

	// Volumes: /nix/store first, then --project, then --file entries.
	volumes := []config.VolumeConfig{
		{Source: "/nix/store", Target: "/nix/store", Mode: "ro"},
	}
	if project != "" {
		parts := strings.SplitN(project, ":", 2)
		src, err := expandPath(parts[0])
		if err != nil {
			return fmt.Errorf("--project: %w", err)
		}
		tgt := "/workspace"
		if len(parts) == 2 && parts[1] != "" {
			tgt = parts[1]
		}
		volumes = append(volumes, config.VolumeConfig{Source: src, Target: tgt, Mode: "rw"})
	}
	for _, f := range files {
		parts := strings.SplitN(f, ":", 3)
		if len(parts) < 2 {
			return fmt.Errorf("--file %q: expected host:container[:mode]", f)
		}
		src, err := expandPath(parts[0])
		if err != nil {
			return fmt.Errorf("--file %q: %w", f, err)
		}
		mode := "ro"
		if len(parts) == 3 {
			mode = parts[2]
		}
		volumes = append(volumes, config.VolumeConfig{Source: src, Target: parts[1], Mode: mode})
	}

	// Environment: TERM is required for tmux; user values override defaults.
	env := map[string]string{"TERM": "xterm-256color"}
	for _, e := range envVars {
		k, v, ok := strings.Cut(e, "=")
		if !ok {
			return fmt.Errorf("--env %q: expected KEY=VALUE", e)
		}
		env[k] = v
	}

	// Home: persistent by default, keyed on agent name.
	homeSrc := homePath
	homeModeStr := "persistent"
	if ephemeral {
		homeModeStr = "ephemeral"
		homeSrc = ""
	} else if homeSrc == "" {
		h, err := os.UserHomeDir()
		if err != nil {
			return fmt.Errorf("home dir: %w", err)
		}
		homeSrc = filepath.Join(h, ".local", "share", "graft", "sessions", name)
	}

	file := &config.File{
		Version: 1,
		Name:    name,
		Config: config.Config{
			Runtime: config.RuntimeConfig{
				Mode:     "rootfs-store",
				Command:  command,
				Packages: pkgs,
			},
			Service: config.ServiceConfig{
				Type:       "notify",
				Restart:    "on-failure",
				RestartSec: "5s",
			},
			Container: config.ContainerConfig{
				Environment: env,
			},
			Filesystem: config.FilesystemConfig{
				Volumes: volumes,
			},
			Home: config.HomeConfig{
				Mode:   homeModeStr,
				Source: homeSrc,
			},
		},
	}
	return startContainer(file)
}

// parseAgentConfig parses the same flags as 'graft agent start' and returns
// a ready-to-use config.File.  It is shared by graftAgentSave.
func parseAgentConfig(args []string) (*config.File, error) {
	fs := flag.NewFlagSet("agent", flag.ContinueOnError)
	var packages flagSlice
	var files flagSlice
	var envVars flagSlice
	var project, homePath, cmdOverride string
	var ephemeral bool

	fs.Var(&packages, "package", "nix `package` to include in PATH (repeatable)")
	fs.Var(&packages, "p", "nix package (shorthand for --package)")
	fs.Var(&files, "file", "`host:container[:mode]` bind-mount (repeatable); mode defaults to ro")
	fs.Var(&files, "f", "bind-mount (shorthand for --file)")
	fs.Var(&envVars, "env", "`KEY=VALUE` environment variable (repeatable)")
	fs.Var(&envVars, "e", "environment variable (shorthand for --env)")
	fs.StringVar(&project, "project", "", "project `dir` to mount at /workspace (host-path[:container-path])")
	fs.StringVar(&homePath, "home", "", "persistent home `path` (default: ~/.local/share/graft/sessions/<name>)")
	fs.StringVar(&cmdOverride, "cmd", "", "override agent entry-point `binary` (default: first --package name)")
	fs.BoolVar(&ephemeral, "ephemeral", false, "use ephemeral home (wiped on stop) instead of persistent")
	fs.Usage = func() {
		fmt.Fprint(os.Stderr, "Usage: graft agent save <name> [flags]\n\nFlags:\n")
		fs.PrintDefaults()
	}

	var name string
	flagArgs := args
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		name = args[0]
		flagArgs = args[1:]
	}
	if err := fs.Parse(flagArgs); err != nil {
		return nil, err
	}
	if name == "" {
		switch len(fs.Args()) {
		case 1:
			name = fs.Args()[0]
		case 0:
			fs.Usage()
			return nil, errors.New("agent save needs exactly one <name>")
		default:
			fs.Usage()
			return nil, errors.New("agent save: unexpected extra arguments")
		}
	} else if len(fs.Args()) != 0 {
		fs.Usage()
		return nil, errors.New("agent save: unexpected extra arguments after name")
	}

	agentBin := cmdOverride
	if agentBin == "" {
		if len(packages) == 0 {
			return nil, errors.New("--package/-p or --cmd is required")
		}
		agentBin = filepath.Base(string(packages[0]))
	}
	command := []string{"tmux", "new-session", "-A", "-s", "main", agentBin}

	pkgs := []string{"tmux"}
	for _, p := range packages {
		if p != "tmux" {
			pkgs = append(pkgs, p)
		}
	}

	volumes := []config.VolumeConfig{
		{Source: "/nix/store", Target: "/nix/store", Mode: "ro"},
	}
	if project != "" {
		parts := strings.SplitN(project, ":", 2)
		src, err := expandPath(parts[0])
		if err != nil {
			return nil, fmt.Errorf("--project: %w", err)
		}
		tgt := "/workspace"
		if len(parts) == 2 && parts[1] != "" {
			tgt = parts[1]
		}
		volumes = append(volumes, config.VolumeConfig{Source: src, Target: tgt, Mode: "rw"})
	}
	for _, f := range files {
		parts := strings.SplitN(f, ":", 3)
		if len(parts) < 2 {
			return nil, fmt.Errorf("--file %q: expected host:container[:mode]", f)
		}
		src, err := expandPath(parts[0])
		if err != nil {
			return nil, fmt.Errorf("--file %q: %w", f, err)
		}
		mode := "ro"
		if len(parts) == 3 {
			mode = parts[2]
		}
		volumes = append(volumes, config.VolumeConfig{Source: src, Target: parts[1], Mode: mode})
	}

	env := map[string]string{"TERM": "xterm-256color"}
	for _, e := range envVars {
		k, v, ok := strings.Cut(e, "=")
		if !ok {
			return nil, fmt.Errorf("--env %q: expected KEY=VALUE", e)
		}
		env[k] = v
	}

	homeSrc := homePath
	homeModeStr := "persistent"
	if ephemeral {
		homeModeStr = "ephemeral"
		homeSrc = ""
	} else if homeSrc == "" {
		h, err := os.UserHomeDir()
		if err != nil {
			return nil, fmt.Errorf("home dir: %w", err)
		}
		homeSrc = filepath.Join(h, ".local", "share", "graft", "sessions", name)
	}

	return &config.File{
		Version: 1,
		Name:    name,
		Config: config.Config{
			Runtime: config.RuntimeConfig{
				Mode:     "rootfs-store",
				Command:  command,
				Packages: pkgs,
			},
			Service: config.ServiceConfig{
				Type:       "notify",
				Restart:    "on-failure",
				RestartSec: "5s",
			},
			Container: config.ContainerConfig{
				Environment: env,
			},
			Filesystem: config.FilesystemConfig{
				Volumes: volumes,
			},
			Home: config.HomeConfig{
				Mode:   homeModeStr,
				Source: homeSrc,
			},
		},
	}, nil
}

// graftAgentSave saves an agent config to ~/.config/graft/agents/<name>.toml
// so it can be started later with just: graft <name>
func graftAgentSave(args []string) error {
	file, err := parseAgentConfig(args)
	if err != nil {
		return err
	}
	dir := agentConfigDir()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create agent config dir: %w", err)
	}
	path := filepath.Join(dir, file.Name+".toml")
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	if err := toml.NewEncoder(f).Encode(file); err != nil {
		return err
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: saved %q → %s\n", file.Name, path)
	_, _ = fmt.Fprintf(os.Stdout, "graft: start with:  graft %s\n", file.Name)
	return nil
}

// configRoot returns the directory graft uses to look up TOML files by name.
// Override with GRAFT_CONFIG_ROOT env var.
// Default: the directory that contains the user's graft config file.
func configRoot() string {
	if d := os.Getenv("GRAFT_CONFIG_ROOT"); d != "" {
		return d
	}
	return filepath.Dir(defaultConfigPath())
}

// agentConfigDir returns the directory where named agent configs are stored.
func agentConfigDir() string {
	if d, err := os.UserConfigDir(); err == nil {
		return filepath.Join(d, "graft", "agents")
	}
	h, _ := os.UserHomeDir()
	return filepath.Join(h, ".config", "graft", "agents")
}

// graftNamedAgent implements 'graft <name>':
//  1. If the container is already running → attach immediately.
//  2. Otherwise find <name>.toml in config root (or saved agents dir),
//     resolve parents, start the container.
//  3. If the command is interactive (uses tmux) → attach after start.
func graftNamedAgent(name string, _ []string) error {
	// 1. Already running?
	running, err := isContainerRunning(name)
	if err != nil {
		return err
	}
	if running {
		return graftAttach([]string{name})
	}

	// 2. Find the config file.
	configPath := ""
	for _, dir := range []string{configRoot(), agentConfigDir()} {
		candidate := filepath.Join(dir, name+".toml")
		if _, statErr := os.Stat(candidate); statErr == nil {
			configPath = candidate
			break
		}
	}
	if configPath == "" {
		return fmt.Errorf("unknown command %q\n" +
			"  no TOML found in GRAFT_CONFIG_ROOT (%s) or %s\n" +
			"  hint: set GRAFT_CONFIG_ROOT or create %s/%s.toml",
			name, configRoot(), agentConfigDir(), configRoot(), name)
	}

	// 3. Resolve parents and start.
	file, err := config.LoadResolved(configPath, []string{configRoot()})
	if err != nil {
		return err
	}
	if err := startContainer(file); err != nil {
		return err
	}

	// 4. Attach if the command is interactive (tmux-based).
	if isInteractiveCommand(file.Config.Runtime.Command) {
		time.Sleep(500 * time.Millisecond)
		return graftAttach([]string{name})
	}
	return nil
}

// isInteractiveCommand returns true if the command starts a tmux session.
func isInteractiveCommand(cmd []string) bool {
	for _, arg := range cmd {
		if arg == "tmux" {
			return true
		}
	}
	return false
}

// isContainerRunning returns true if a graft-managed container with the given
// name is currently running.
func isContainerRunning(name string) (bool, error) {
	out, err := exec.Command(
		"podman", "ps",
		"--filter", "name=^"+name+"$",
		"--filter", "label=managed-by=graft",
		"--format", "{{.Names}}",
	).Output()
	if err != nil {
		return false, fmt.Errorf("podman ps: %w", err)
	}
	return strings.TrimSpace(string(out)) != "", nil
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

func buildRuntimeEnv(packageNames []string) (string, error) {
	for _, packageName := range packageNames {
		if !packageNamePattern.MatchString(packageName) {
			return "", fmt.Errorf("invalid package name %q; expected only letters, numbers, dot, underscore, plus, or dash", packageName)
		}
	}
	expr := `let
  pkgs = import (builtins.getFlake "nixpkgs") { system = builtins.currentSystem; };
  packageNames = [` + nixStringList(packageNames) + ` ];
  packages = map (name:
    if builtins.hasAttr name pkgs then builtins.getAttr name pkgs
    else throw "unknown package ${name}"
  ) packageNames;
in pkgs.buildEnv { name = "graft-runtime"; paths = packages; }`
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
	value = strings.ReplaceAll(value, `\`, `\\`)
	value = strings.ReplaceAll(value, `"`, `\"`)
	value = strings.ReplaceAll(value, `${`, `\${`)
	return `"` + value + `"`
}
