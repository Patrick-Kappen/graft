package cli

import (
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

var workspaceSkipDirs = []string{".git", ".jj", ".go", ".direnv", "result", "node_modules"}

func shouldSkipDir(name string, skipDirs []string) bool {
	for _, dir := range skipDirs {
		if name == dir {
			return true
		}
	}
	return false
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
		if entry.IsDir() && len(skipDirs) > 0 && shouldSkipDir(name, skipDirs) {
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

// afterDash returns args after a leading "--" separator, or args unchanged.
func afterDash(args []string) []string {
	if len(args) > 0 && args[0] == "--" {
		return args[1:]
	}
	return args
}

// applyWorkspace copies all files from candidate (container output) back to
// dest (the original host directory). New and modified files are applied;
// files deleted inside the container are left untouched on the host.
func applyWorkspace(candidate, dest string, skipDirs []string) error {
	return copyTree(candidate, dest, skipDirs)
}

// promptUser prints a question and reads a y/yes answer from stdin.
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
