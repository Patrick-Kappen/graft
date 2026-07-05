package cli

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/zerodawn1990/graft/internal/config"
)

// applyHomeEnvVars sets HOME and XDG_* variables in env for the given
// container-side home directory target.
func applyHomeEnvVars(env map[string]string, target string) {
	env["HOME"] = target
	env["XDG_CONFIG_HOME"] = filepath.Join(target, ".config")
	env["XDG_CACHE_HOME"] = filepath.Join(target, ".cache")
	env["XDG_DATA_HOME"] = filepath.Join(target, ".local", "share")
	env["XDG_STATE_HOME"] = filepath.Join(target, ".local", "state")
}

// mountHomeDir creates homeDir on the host if needed, appends a rw volume bind
// to cfg, and sets HOME / XDG vars inside the container config.
func mountHomeDir(cfg *config.Config, homeDir, homeTarget string) error {
	if err := os.MkdirAll(homeDir, 0o755); err != nil {
		return err
	}
	cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes, config.VolumeConfig{Source: homeDir, Target: homeTarget, Mode: "rw"})
	applyHomeEnvVars(cfg.Container.Environment, homeTarget)
	return nil
}

// mountPersistentHome resolves cfg.Home.Source, creates it on the host, and
// delegates to mountHomeDir.
func mountPersistentHome(cfg *config.Config, homeTarget string) error {
	homeSrc, err := expandPath(cfg.Home.Source)
	if err != nil {
		return fmt.Errorf("home.source: %w", err)
	}
	if homeSrc == "" {
		return errors.New("home.source is required when home.mode = \"persistent\"")
	}
	return mountHomeDir(cfg, homeSrc, homeTarget)
}

// prepareTransientIsolation sets up home, workspace, and shadow mount
// isolation for transient (dev-path) containers. It returns the modified
// config and review/promote callbacks to run after the container exits.
func prepareTransientIsolation(cfg config.Config, workDir string) (config.Config, func() error, func() error, error) {
	prepared := cfg
	var reviewFns, promoteFns []func() error

	// chainCallbacks builds the final review and promote closures from the
	// accumulated slices so that home-session, shadow, and workspace callbacks compose.
	chainCallbacks := func() (func() error, func() error) {
		rev := func() error {
			for _, fn := range reviewFns {
				if err := fn(); err != nil {
					return err
				}
			}
			return nil
		}
		pro := func() error {
			for _, fn := range promoteFns {
				if err := fn(); err != nil {
					return err
				}
			}
			return nil
		}
		return rev, pro
	}

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
		homeTarget = "/home/user"
	}
	switch homeMode {
	case "ephemeral":
		if err := mountHomeDir(&prepared, filepath.Join(workDir, "home"), homeTarget); err != nil {
			return config.Config{}, nil, nil, err
		}
	case "persistent":
		// Persistent home survives across runs.
		if err := mountPersistentHome(&prepared, homeTarget); err != nil {
			return config.Config{}, nil, nil, err
		}
	case "session":
		// Session mode: the source dir is copied to a temp dir for this run.
		// Changes are reviewed and optionally promoted back at session end.
		homeSrc, err := expandPath(prepared.Home.Source)
		if err != nil {
			return config.Config{}, nil, nil, fmt.Errorf("home.source: %w", err)
		}
		if homeSrc == "" {
			return config.Config{}, nil, nil, errors.New("home.source is required when home.mode = \"session\"")
		}
		sessionDir := filepath.Join(workDir, "home-session")
		if _, statErr := os.Stat(homeSrc); os.IsNotExist(statErr) {
			// First run: source does not exist yet — start with an empty home.
			if err := os.MkdirAll(sessionDir, 0o755); err != nil {
				return config.Config{}, nil, nil, err
			}
		} else {
			if err := copyTree(homeSrc, sessionDir, nil); err != nil {
				return config.Config{}, nil, nil, fmt.Errorf("home session copy: %w", err)
			}
		}
		if err := mountHomeDir(&prepared, sessionDir, homeTarget); err != nil {
			return config.Config{}, nil, nil, err
		}
		if prepared.Home.Review == "diff" {
			src, sess := homeSrc, sessionDir
			reviewFns = append(reviewFns, func() error {
				return printWorkspaceDiff(src, sess, nil)
			})
		}
		switch prepared.Home.Promote {
		case "auto":
			src, sess := homeSrc, sessionDir
			promoteFns = append(promoteFns, func() error {
				if err := os.MkdirAll(src, 0o755); err != nil {
					return err
				}
				_, _ = fmt.Fprintf(os.Stderr, "graft: applying home session changes to %s\n", src)
				return applyWorkspace(sess, src, nil)
			})
		case "prompt":
			src, sess := homeSrc, sessionDir
			promoteFns = append(promoteFns, func() error {
				ok, err := promptUser("Apply home session changes to " + src + "?")
				if err != nil || !ok {
					return err
				}
				if err := os.MkdirAll(src, 0o755); err != nil {
					return err
				}
				return applyWorkspace(sess, src, nil)
			})
		}
	}

	// Set up shadow mounts (independent of home mode; work for all dev runs).
	for i, sm := range prepared.Home.Shadow {
		hostPath, err := expandPath(sm.Host)
		if err != nil {
			return config.Config{}, nil, nil, fmt.Errorf("home.shadow[%d].host: %w", i, err)
		}
		if sm.Container == "" {
			return config.Config{}, nil, nil, fmt.Errorf("home.shadow[%d].container is required", i)
		}
		shadowDir := filepath.Join(workDir, "shadow", shadowDirName(sm.Container))
		if err := os.MkdirAll(shadowDir, 0o755); err != nil {
			return config.Config{}, nil, nil, err
		}
		if hostPath != "" {
			if _, statErr := os.Stat(hostPath); statErr == nil {
				if err := copyTree(hostPath, shadowDir, nil); err != nil {
					return config.Config{}, nil, nil, fmt.Errorf("shadow mount copy for %s: %w", sm.Container, err)
				}
			}
		}
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes, config.VolumeConfig{
			Source: shadowDir,
			Target: sm.Container,
			Mode:   "z",
		})
		if hostPath != "" {
			src, sess, cpath := hostPath, shadowDir, sm.Container
			reviewFns = append(reviewFns, func() error {
				_, _ = fmt.Fprintf(os.Stderr, "=== shadow diff %s ===\n", cpath)
				return printWorkspaceDiff(src, sess, nil)
			})
			promote := src
			promote2 := sess
			promote3 := cpath
			promoteFns = append(promoteFns, func() error {
				ok, err := promptUser("Apply shadow changes for " + promote3 + " to " + promote + "?")
				if err != nil || !ok {
					return err
				}
				if err := os.MkdirAll(promote, 0o755); err != nil {
					return err
				}
				return applyWorkspace(promote2, promote, nil)
			})
		}
	}

	if prepared.Workspace.Mode == "" || prepared.Workspace.Mode == "none" {
		review, promote := chainCallbacks()
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
		abs, d, skip := absSource, dest, skipDirs
		reviewFns = append(reviewFns, func() error {
			return printWorkspaceDiff(abs, d, skip)
		})
	}
	switch prepared.Workspace.Promote {
	case "auto":
		abs, d, skip := absSource, dest, skipDirs
		promoteFns = append(promoteFns, func() error {
			_, _ = fmt.Fprintf(os.Stderr, "graft: applying workspace changes to %s\n", abs)
			return applyWorkspace(d, abs, skip)
		})
	case "prompt":
		abs, d, skip := absSource, dest, skipDirs
		promoteFns = append(promoteFns, func() error {
			ok, err := promptUser("Apply workspace changes to " + abs + "?")
			if err != nil || !ok {
				return err
			}
			return applyWorkspace(d, abs, skip)
		})
	}
	review, promote := chainCallbacks()
	return prepared, review, promote, nil
}
