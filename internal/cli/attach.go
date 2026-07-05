package cli

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"time"
)

// containerAttachConfig holds per-container attach settings read from Podman labels.
type containerAttachConfig struct {
	tmuxSession string
	shell       string
	startDelay  time.Duration
}

// readAttachConfig reads graft.attach.* labels from a running container via
// podman inspect. Falls back to sensible defaults when labels are absent.
func readAttachConfig(name, host string) containerAttachConfig {
	cfg := containerAttachConfig{tmuxSession: "main", shell: "sh", startDelay: 500 * time.Millisecond}
	cmd := remoteExec(host, false, "podman", "inspect", name,
		"--format", `{{index .Config.Labels "graft.attach.tmux-session"}}|{{index .Config.Labels "graft.attach.shell"}}|{{index .Config.Labels "graft.attach.start-delay"}}`)
	out, err := cmd.Output()
	if err != nil {
		return cfg
	}
	parts := strings.SplitN(strings.TrimSpace(string(out)), "|", 3)
	if len(parts) >= 1 && parts[0] != "" {
		cfg.tmuxSession = parts[0]
	}
	if len(parts) >= 2 && parts[1] != "" {
		cfg.shell = parts[1]
	}
	if len(parts) >= 3 && parts[2] != "" {
		if d, err := time.ParseDuration(parts[2]); err == nil {
			cfg.startDelay = d
		}
	}
	return cfg
}

// graftAttach attaches to a running container's tmux session. If no session
// exists, it starts a new one. If tmux is not available, falls back to a shell.
// The tmux session name and fallback shell are configurable via [config.attach]
// in the container's TOML; defaults are "main" and "sh".
func graftAttach(args []string, host string) error {
	if len(args) < 1 {
		return errors.New("attach needs: <container-name>")
	}
	name := args[0]
	ac := readAttachConfig(name, host)

	// 1. Attach to an existing tmux session.
	attach := remoteExec(host, true, "podman", "exec", "-it", name, "tmux", "attach-session", "-t", ac.tmuxSession)
	attach.Stdin, attach.Stdout, attach.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := attach.Run(); err == nil {
		return nil
	}

	// 2. No existing session — start a new tmux session.
	_, _ = fmt.Fprintf(os.Stderr, "graft: no tmux session %q found, starting new session\n", ac.tmuxSession)
	newSession := remoteExec(host, true, "podman", "exec", "-it", name, "tmux", "new-session", "-s", ac.tmuxSession)
	newSession.Stdin, newSession.Stdout, newSession.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := newSession.Run(); err == nil {
		return nil
	}

	// 3. tmux not available — fall back to configured shell.
	_, _ = fmt.Fprintf(os.Stderr, "graft: tmux not available, falling back to %s\n", ac.shell)
	shell := remoteExec(host, true, "podman", "exec", "-it", name, ac.shell)
	shell.Stdin, shell.Stdout, shell.Stderr = os.Stdin, os.Stdout, os.Stderr
	return shell.Run()
}

// graftList lists running containers that carry the managed-by=graft label.
func graftList(_ []string, host string) error {
	cmd := remoteExec(host, false, "podman", "ps",
		"--filter", "label=managed-by=graft",
		"--format", `table {{.Names}}\t{{.Status}}\t{{.RunningFor}}`)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}

// graftLogs shows journalctl output for a graft-managed service.
// With --denied it filters to lines containing "DENIED" (blocked egress).
func graftLogs(args []string, host string) error {
	if len(args) == 0 {
		return errors.New("logs needs: <container-name> [--denied]")
	}
	name := args[0]
	journalArgs := append(journalScopeFlag(), "-u", name+".service", "--no-pager", "-o", "cat")
	if len(args) > 1 && args[1] == "--denied" {
		journalArgs = append(journalArgs, "-g", "DENIED")
	}
	cmd := remoteExec(host, false, "journalctl", journalArgs...)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}
