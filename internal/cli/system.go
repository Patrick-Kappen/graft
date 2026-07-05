package cli

import (
	"os"
	"os/exec"
	"strings"
)

// remoteExec SSHs to host and runs graft with the given args. If interactive
// is true, SSH gets a pseudo-tty (-t). Authentication relies on the caller's
// SSH agent / known_hosts — the same as any normal SSH connection.
func remoteExec(host string, interactive bool, args ...string) error {
	sshArgs := []string{"-o", "BatchMode=yes"}
	if interactive {
		sshArgs = append(sshArgs, "-t")
	}
	sshArgs = append(sshArgs, host, "graft")
	sshArgs = append(sshArgs, args...)
	cmd := exec.Command("ssh", sshArgs...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	return cmd.Run()
}

// extractFlag removes --flag value (or --flag=value) from args and returns the
// value and the remaining args. Returns ("", args) if the flag is absent.
func extractFlag(flag string, args []string) (string, []string) {
	prefix := flag + "="
	for i, a := range args {
		if a == flag && i+1 < len(args) {
			rest := make([]string, 0, len(args)-2)
			rest = append(rest, args[:i]...)
			rest = append(rest, args[i+2:]...)
			return args[i+1], rest
		}
		if strings.HasPrefix(a, prefix) {
			rest := make([]string, 0, len(args)-1)
			rest = append(rest, args[:i]...)
			rest = append(rest, args[i+1:]...)
			return a[len(prefix):], rest
		}
	}
	return "", args
}
