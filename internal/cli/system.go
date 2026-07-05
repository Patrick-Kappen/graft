package cli

import (
	"os"
	"os/exec"
	"strings"
)

// isSystemScope reports whether GRAFT_SYSTEMD_SCOPE=system is set.
func isSystemScope() bool {
	return os.Getenv("GRAFT_SYSTEMD_SCOPE") == "system"
}

// journalScopeFlag returns the --user flag for journalctl when not in system scope.
func journalScopeFlag() []string {
	if isSystemScope() {
		return nil
	}
	return []string{"--user"}
}

// shellescape returns a single-quoted, shell-safe version of s.
func shellescape(s string) string {
	return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'"
}

// remoteExec returns an *exec.Cmd that runs name+args locally or via SSH on host.
// interactive=true passes -t to ssh for TTY allocation (e.g. for attach).
func remoteExec(host string, interactive bool, name string, args ...string) *exec.Cmd {
	if host == "" {
		return exec.Command(name, args...)
	}
	parts := make([]string, len(args)+1)
	parts[0] = shellescape(name)
	for i, a := range args {
		parts[i+1] = shellescape(a)
	}
	sshArgs := make([]string, 0, 3)
	if interactive {
		sshArgs = append(sshArgs, "-t")
	}
	sshArgs = append(sshArgs, host, strings.Join(parts, " "))
	return exec.Command("ssh", sshArgs...)
}

// extractFlag removes --flag value (or --flag=value) from args and returns
// the value and the remaining args slice. Returns ("", args) if not present.
func extractFlag(args []string, flag string) (string, []string) {
	for i, a := range args {
		if a == flag && i+1 < len(args) {
			rest := append(args[:i:i], args[i+2:]...)
			return args[i+1], rest
		}
		if strings.HasPrefix(a, flag+"=") {
			rest := append(args[:i:i], args[i+1:]...)
			return strings.TrimPrefix(a, flag+"="), rest
		}
	}
	return "", args
}

// systemctlScope runs systemctl with user or system scope, locally or via SSH.
// Set GRAFT_SYSTEMD_SCOPE=system for NixOS system-target units;
// defaults to user scope (--user) for Home Manager / rootless units.
func systemctlScope(host string, args ...string) error {
	var scopeArgs []string
	if isSystemScope() {
		scopeArgs = args
	} else {
		scopeArgs = append([]string{"--user"}, args...)
	}
	cmd := remoteExec(host, false, "systemctl", scopeArgs...)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}
