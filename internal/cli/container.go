package cli

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/Patrick-Kappen/graft/internal/config"
	"github.com/Patrick-Kappen/graft/internal/quadlet"
	graftruntime "github.com/Patrick-Kappen/graft/internal/runtime"
)

func graftUp(args []string) error {
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
	return graftRun([]string{path})
}

func graftRun(args []string) error {
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

// graftStart parses args and delegates to startContainer.
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

// startContainer writes Quadlet units, sets up shadow mounts, writes session
// meta, and starts the systemd service for a managed (detached) container.
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

	homeMode := cfg.Home.Mode
	if cfg.Home.Ephemeral && homeMode == "" {
		homeMode = "ephemeral"
	}
	homeTarget := cfg.Home.Target
	if homeTarget == "" {
		homeTarget = "/home/user"
	}
	setHomeEnv := func() {
		cfg.Container.Environment["HOME"] = homeTarget
		cfg.Container.Environment["XDG_CONFIG_HOME"] = filepath.Join(homeTarget, ".config")
		cfg.Container.Environment["XDG_CACHE_HOME"] = filepath.Join(homeTarget, ".cache")
		cfg.Container.Environment["XDG_DATA_HOME"] = filepath.Join(homeTarget, ".local", "share")
		cfg.Container.Environment["XDG_STATE_HOME"] = filepath.Join(homeTarget, ".local", "state")
	}

	sessionDir := filepath.Join(userDataDir(), "graft", "sessions", name)

	// Clean up any stale session left by a previous crash before starting.
	if meta, err := readSessionMeta(name); err == nil {
		_, _ = fmt.Fprintf(os.Stderr,
			"graft: found stale session for %q (started %s), cleaning up\n",
			name, meta.StartedAt)
		_ = handleSessionStop(name)
	}

	switch homeMode {
	case "ephemeral":
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			return err
		}
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes,
			config.VolumeConfig{Source: sessionDir, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
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
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes,
			config.VolumeConfig{Source: homeSrc, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
	}

	// Set up shadow mounts if configured.
	var shadows []shadowMeta
	if len(cfg.Home.Shadow) > 0 {
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			return err
		}
		var err error
		shadows, err = setupShadowMounts(&cfg, sessionDir)
		if err != nil {
			return err
		}
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

	// Write session meta before starting so we can recover on crash.
	if cfg.Home.Session || len(shadows) > 0 {
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			return err
		}
		if err := writeSessionMeta(sessionMeta{
			ContainerName: name,
			StartedAt:     time.Now().UTC().Format(time.RFC3339),
			Shadows:       shadows,
		}); err != nil {
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

// graftStop stops a running container, handles session cleanup, and removes the
// runtime Quadlet unit files.
func graftStop(args []string) error {
	if len(args) != 1 {
		return errors.New("stop needs: <container-name>")
	}
	name := args[0]

	if err := handleSessionStop(name); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "graft: warning: session cleanup: %v\n", err)
	}

	_ = graftruntime.SystemctlUser("stop", name+".service")

	runtimeDir := graftruntime.RuntimeDir()
	quadletDir := filepath.Join(runtimeDir, "containers", "systemd")
	for _, ext := range []string{".container", ".network", ".volume"} {
		_ = os.Remove(filepath.Join(quadletDir, name+ext))
	}
	_ = graftruntime.SystemctlUser("daemon-reload")
	_, _ = fmt.Fprintf(os.Stdout, "graft: stopped %s\n", name)
	return nil
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

// graftStartOrAttach implements 'graft <name>':
//  1. If the container is already running → attach immediately.
//  2. Otherwise find <name>.toml in configRoot, resolve parents, start.
//  3. If the command is interactive (uses tmux) → attach after start.
func graftStartOrAttach(name string) error {
	running, err := isContainerRunning(name)
	if err != nil {
		return err
	}
	if running {
		return graftAttach([]string{name})
	}

	configPath := filepath.Join(configRoot(), name+".toml")
	if _, err := os.Stat(configPath); err != nil {
		return fmt.Errorf("unknown command %q\n"+
			"  no TOML found: %s\n"+
			"  hint: set GRAFT_CONFIG_ROOT or create %s/%s.toml",
			name, configPath, configRoot(), name)
	}

	file, err := config.LoadResolved(configPath, []string{configRoot()})
	if err != nil {
		return err
	}
	if err := startContainer(file); err != nil {
		return err
	}
	if isInteractiveCommand(file.Config.Runtime.Command) {
		time.Sleep(500 * time.Millisecond)
		return graftAttach([]string{name})
	}
	return nil
}

func isInteractiveCommand(cmd []string) bool {
	for _, arg := range cmd {
		if arg == "tmux" {
			return true
		}
	}
	return false
}

func configRoot() string {
	if d := os.Getenv("GRAFT_CONFIG_ROOT"); d != "" {
		return d
	}
	return filepath.Dir(defaultConfigPath())
}
