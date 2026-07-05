package cli

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
)

// graftAttach attaches to a running container's tmux session. If no session
// named "main" exists, it starts a new interactive tmux session inside the
// container.
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

// graftLogs shows the logs of a running or stopped container.
// Pass -f or --follow to stream logs continuously.
func graftLogs(args []string) error {
	if len(args) < 1 {
		return errors.New("logs needs: <container-name>")
	}
	podmanArgs := []string{"logs"}
	name := ""
	for _, a := range args {
		switch a {
		case "-f", "--follow":
			podmanArgs = append(podmanArgs, "-f")
		default:
			if name == "" {
				name = a
			}
		}
	}
	if name == "" {
		return errors.New("logs needs: <container-name>")
	}
	podmanArgs = append(podmanArgs, name)
	cmd := exec.Command("podman", podmanArgs...)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}
