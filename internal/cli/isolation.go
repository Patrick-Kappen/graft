package cli

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/Patrick-Kappen/graft/internal/config"
)

// prepareTransientIsolation sets up ephemeral/persistent home, shadow mounts,
// and workspace copy for a transient (run/run-rootfs) container. It returns the
// augmented config plus review and promote callbacks that are called after the
// container exits.
func prepareTransientIsolation(cfg config.Config, workDir string) (config.Config, func() error, func() error, error) {
	var reviewFns, promoteFns []func() error
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
		homeTarget = "/home/user"
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
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes,
			config.VolumeConfig{Source: homeDir, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
	case "persistent":
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
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes,
			config.VolumeConfig{Source: homeSrc, Target: homeTarget, Mode: "rw"})
		setHomeEnv()
	}

	// Shadow mounts: copy each source into a per-run shadow dir, mount it, then
	// wire up review (diff) and promote (copy-back) callbacks.
	for _, shadow := range prepared.Home.Shadow {
		src, err := expandPath(shadow.Source)
		if err != nil {
			return config.Config{}, nil, nil, fmt.Errorf("home.shadow source %q: %w", shadow.Source, err)
		}
		target := shadow.Target
		if target == "" {
			target = src
		}
		shadowDir := filepath.Join(workDir, "shadow", shadowDirName(src))
		if err := copyTree(src, shadowDir, nil); err != nil {
			return config.Config{}, nil, nil, fmt.Errorf("shadow mount %q: %w", src, err)
		}
		prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes,
			config.VolumeConfig{Source: shadowDir, Target: target, Mode: "rw"})

		srcCopy, shadowCopy := src, shadowDir
		reviewFns = append(reviewFns, func() error {
			return printWorkspaceDiff(srcCopy, shadowCopy, nil)
		})
		promoteFns = append(promoteFns, func() error {
			_, _ = fmt.Fprintf(os.Stderr, "graft: promoting shadow %s → %s\n", shadowCopy, srcCopy)
			return applyWorkspace(shadowCopy, srcCopy, nil)
		})
	}

	// Workspace copy.
	if prepared.Workspace.Mode == "" || prepared.Workspace.Mode == "none" {
		return prepared, chainFns(reviewFns), chainFns(promoteFns), nil
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
	prepared.Filesystem.Volumes = append(prepared.Filesystem.Volumes,
		config.VolumeConfig{Source: dest, Target: wsTarget, Mode: "rw"})
	if prepared.Container.WorkingDir == "" {
		prepared.Container.WorkingDir = wsTarget
	}
	if prepared.Workspace.Review == "diff" {
		reviewFns = append(reviewFns, func() error {
			return printWorkspaceDiff(absSource, dest, skipDirs)
		})
	}
	switch prepared.Workspace.Promote {
	case "auto":
		promoteFns = append(promoteFns, func() error {
			_, _ = fmt.Fprintf(os.Stderr, "graft: applying workspace changes to %s\n", absSource)
			return applyWorkspace(dest, absSource, skipDirs)
		})
	case "prompt":
		promoteFns = append(promoteFns, func() error {
			ok, err := promptUser("Apply workspace changes to " + absSource + "?")
			if err != nil || !ok {
				return err
			}
			return applyWorkspace(dest, absSource, skipDirs)
		})
	}

	return prepared, chainFns(reviewFns), chainFns(promoteFns), nil
}

// chainFns returns a single function that calls each fn in order, stopping on
// the first error.
func chainFns(fns []func() error) func() error {
	return func() error {
		for _, fn := range fns {
			if err := fn(); err != nil {
				return err
			}
		}
		return nil
	}
}
