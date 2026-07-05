package runtime

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"time"
)

type TransientInput struct {
	Quadlet   string
	UnitStem  string
	KeepFiles bool
}

func RunTransient(input TransientInput) error {
	runtimeDir := RuntimeDir()
	quadletDir := filepath.Join(runtimeDir, "containers", "systemd")
	runID := input.UnitStem
	if runID == "" {
		runID = fmt.Sprintf("graft-%d-%d", os.Getpid(), time.Now().UnixNano())
	}
	unitName := runID + ".service"
	quadletPath := filepath.Join(quadletDir, runID+".container")

	if err := os.MkdirAll(quadletDir, 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(quadletPath, []byte(input.Quadlet), 0o644); err != nil {
		return err
	}
	defer func() {
		_ = SystemctlUser("stop", unitName)
		if !input.KeepFiles {
			_ = os.Remove(quadletPath)
		}
		_ = SystemctlUser("daemon-reload")
	}()

	if err := SystemctlUser("daemon-reload"); err != nil {
		return err
	}
	_ = SystemctlUser("reset-failed", unitName)
	if err := SystemctlUser("start", unitName); err != nil {
		_ = Journal(unitName)
		return err
	}
	return Journal(unitName)
}

func RuntimeDir() string {
	if runtimeDir := os.Getenv("XDG_RUNTIME_DIR"); runtimeDir != "" {
		return runtimeDir
	}
	return filepath.Join("/run/user", strconv.Itoa(os.Getuid()))
}

func SystemctlUser(args ...string) error {
	cmd := exec.Command("systemctl", append([]string{"--user"}, args...)...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

// Systemctl runs systemctl at system scope (no --user).
func Systemctl(args ...string) error {
	cmd := exec.Command("systemctl", args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

func Journal(unitName string) error {
	cmd := exec.Command("journalctl", "--user", "-u", unitName, "--no-pager", "-n", "100", "-o", "cat")
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}
