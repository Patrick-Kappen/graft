package cli

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/Patrick-Kappen/graft/internal/config"
)

// shadowMeta records a single shadow mount within a session.
type shadowMeta struct {
	Source    string `json:"source"`    // original host path
	ShadowDir string `json:"shadowDir"` // per-session copy
	Target    string `json:"target"`    // mount point inside the container
}

// sessionMeta is the JSON sidecar written when a managed container starts.
// Stored at: userDataDir()/graft/sessions/<name>/meta.json
type sessionMeta struct {
	ContainerName string       `json:"containerName"`
	StartedAt     string       `json:"startedAt"`
	Shadows       []shadowMeta `json:"shadows,omitempty"`
}

func sessionMetaPath(name string) string {
	return filepath.Join(userDataDir(), "graft", "sessions", name, "meta.json")
}

func writeSessionMeta(meta sessionMeta) error {
	path := sessionMetaPath(meta.ContainerName)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0o644)
}

func readSessionMeta(name string) (sessionMeta, error) {
	data, err := os.ReadFile(sessionMetaPath(name))
	if err != nil {
		return sessionMeta{}, err
	}
	var meta sessionMeta
	if err := json.Unmarshal(data, &meta); err != nil {
		return sessionMeta{}, err
	}
	return meta, nil
}

// shadowDirName converts a source path to a safe directory name component.
func shadowDirName(source string) string {
	name := strings.ReplaceAll(strings.TrimPrefix(source, "/"), "/", "_")
	if len(name) > 64 {
		name = name[len(name)-64:]
	}
	return name
}

// setupShadowMounts copies each [[home.shadow]] source into a per-session
// shadow directory, appends it as a volume to cfg, and returns the recorded
// metadata slice.
func setupShadowMounts(cfg *config.Config, sessionDir string) ([]shadowMeta, error) {
	var metas []shadowMeta
	for _, shadow := range cfg.Home.Shadow {
		src, err := expandPath(shadow.Source)
		if err != nil {
			return nil, fmt.Errorf("shadow source %q: %w", shadow.Source, err)
		}
		target := shadow.Target
		if target == "" {
			target = src
		}
		shadowDir := filepath.Join(sessionDir, "shadow", shadowDirName(src))
		if err := copyTree(src, shadowDir, nil); err != nil {
			return nil, fmt.Errorf("copying shadow %q: %w", src, err)
		}
		cfg.Filesystem.Volumes = append(cfg.Filesystem.Volumes, config.VolumeConfig{
			Source: shadowDir,
			Target: target,
			Mode:   "rw",
		})
		metas = append(metas, shadowMeta{
			Source:    src,
			ShadowDir: shadowDir,
			Target:    target,
		})
	}
	return metas, nil
}

// handleSessionStop is called when a managed container stops. It warns about
// any unsaved shadow changes and removes the meta.json sidecar.
func handleSessionStop(name string) error {
	path := sessionMetaPath(name)
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return nil
	}
	meta, err := readSessionMeta(name)
	if err == nil && len(meta.Shadows) > 0 {
		_, _ = fmt.Fprintf(os.Stderr,
			"graft: session %q has %d shadow mount(s); use 'graft promote %s' to save changes\n",
			name, len(meta.Shadows), name)
	}
	return os.Remove(path)
}

// graftDiff shows the diff between each shadow mount and its original source.
func graftDiff(args []string) error {
	host, args := extractFlag("--host", args)
	if host != "" {
		return remoteExec(host, false, append([]string{"diff"}, args...)...)
	}
	if len(args) < 1 {
		return errors.New("diff needs: <container-name>")
	}
	name := args[0]
	meta, err := readSessionMeta(name)
	if err != nil {
		return fmt.Errorf("no session data for %q (is the container running with home.session = true?): %w", name, err)
	}
	if len(meta.Shadows) == 0 {
		_, _ = fmt.Fprintf(os.Stdout, "graft: no shadow mounts for %q\n", name)
		return nil
	}
	for _, s := range meta.Shadows {
		if err := printWorkspaceDiff(s.Source, s.ShadowDir, nil); err != nil {
			return err
		}
	}
	return nil
}

// graftPromote copies shadow mount changes back to their original host paths.
func graftPromote(args []string) error {
	host, args := extractFlag("--host", args)
	if host != "" {
		return remoteExec(host, false, append([]string{"promote"}, args...)...)
	}
	if len(args) < 1 {
		return errors.New("promote needs: <container-name>")
	}
	name := args[0]
	meta, err := readSessionMeta(name)
	if err != nil {
		return fmt.Errorf("no session data for %q (is the container running with home.session = true?): %w", name, err)
	}
	if len(meta.Shadows) == 0 {
		_, _ = fmt.Fprintf(os.Stdout, "graft: no shadow mounts for %q\n", name)
		return nil
	}
	for _, s := range meta.Shadows {
		_, _ = fmt.Fprintf(os.Stderr, "graft: promoting %s → %s\n", s.ShadowDir, s.Source)
		if err := applyWorkspace(s.ShadowDir, s.Source, nil); err != nil {
			return err
		}
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: promoted %d shadow mount(s) for %q\n", len(meta.Shadows), name)
	return nil
}

// graftReset re-copies each shadow mount source over the shadow dir, discarding
// any changes the container made since the session started.
func graftReset(args []string) error {
	host, args := extractFlag("--host", args)
	if host != "" {
		return remoteExec(host, false, append([]string{"reset"}, args...)...)
	}
	if len(args) < 1 {
		return errors.New("reset needs: <container-name>")
	}
	name := args[0]
	meta, err := readSessionMeta(name)
	if err != nil {
		return fmt.Errorf("no session data for %q (is the container running with home.session = true?): %w", name, err)
	}
	if len(meta.Shadows) == 0 {
		_, _ = fmt.Fprintf(os.Stdout, "graft: no shadow mounts for %q\n", name)
		return nil
	}
	for _, s := range meta.Shadows {
		_, _ = fmt.Fprintf(os.Stderr, "graft: resetting shadow %s from %s\n", s.ShadowDir, s.Source)
		if err := os.RemoveAll(s.ShadowDir); err != nil {
			return err
		}
		if err := copyTree(s.Source, s.ShadowDir, nil); err != nil {
			return err
		}
	}
	_, _ = fmt.Fprintf(os.Stdout, "graft: reset %d shadow mount(s) for %q\n", len(meta.Shadows), name)
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
