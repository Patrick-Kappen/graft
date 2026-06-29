package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/zerodawn1990/graft/internal/config"
)

func TestAutodetectConfigFindsFirstCandidate(t *testing.T) {
	oldWd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(oldWd) })

	dir := t.TempDir()
	if err := os.Chdir(dir); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"config.toml", ".graft.toml", "graft.toml"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("version = 1\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	path, err := autodetectConfig()
	if err != nil {
		t.Fatalf("autodetectConfig() error = %v", err)
	}
	if path != "graft.toml" {
		t.Fatalf("path = %q, want graft.toml", path)
	}
}

func TestAutodetectConfigMissing(t *testing.T) {
	oldWd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(oldWd) })
	if err := os.Chdir(t.TempDir()); err != nil {
		t.Fatal(err)
	}

	_, err = autodetectConfig()
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "no TOML config found") {
		t.Fatalf("error = %q", err)
	}
}

func TestValidateRunnable(t *testing.T) {
	tests := []struct {
		name    string
		runtime config.RuntimeConfig
		wantErr string
	}{
		{
			name: "valid",
			runtime: config.RuntimeConfig{
				Mode:    "rootfs-store",
				Command: []string{"bash"},
			},
		},
		{
			name: "fragment without mode is not runnable",
			runtime: config.RuntimeConfig{
				Packages: []string{"bashInteractive"},
			},
			wantErr: "config.runtime.mode must be rootfs-store",
		},
		{
			name: "missing command",
			runtime: config.RuntimeConfig{
				Mode: "rootfs-store",
			},
			wantErr: "config.runtime.command must not be empty",
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			err := validateRunnable(test.runtime)
			if test.wantErr == "" {
				if err != nil {
					t.Fatalf("validateRunnable() error = %v", err)
				}
				return
			}
			if err == nil {
				t.Fatal("expected error")
			}
			if !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("error = %q, want containing %q", err, test.wantErr)
			}
		})
	}
}

func TestNixStringEscapesInterpolation(t *testing.T) {
	got := nixString(`a\"b${system}`)
	want := `"a\\\"b\${system}"`
	if got != want {
		t.Fatalf("nixString() = %q, want %q", got, want)
	}
}

func TestBuildRuntimeEnvRejectsInvalidPackageNames(t *testing.T) {
	_, err := buildRuntimeEnv([]string{`bashInteractive\"; builtins.abort "boom"; "`})
	if err == nil {
		t.Fatal("expected invalid package name error")
	}
	if !strings.Contains(err.Error(), "invalid package name") {
		t.Fatalf("error = %q, want invalid package name", err)
	}
}

func TestNixIdentifier(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"my-agent", "my-agent"},
		{"@anthropic-ai/sdk", "anthropic-ai-sdk"},
		{"@scope/my.package", "scope-my-package"},
		{"simple", "simple"},
		{"with spaces", "with-spaces"},
	}
	for _, tt := range tests {
		if got := nixIdentifier(tt.input); got != tt.want {
			t.Errorf("nixIdentifier(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestExpandPath(t *testing.T) {
	home, _ := os.UserHomeDir()
	tests := []struct {
		input string
		want  string
	}{
		{"", ""},
		{"~/foo", home + "/foo"},
		{"/abs/path", "/abs/path"},
	}
	for _, tt := range tests {
		got, err := expandPath(tt.input)
		if err != nil {
			t.Errorf("expandPath(%q) error = %v", tt.input, err)
			continue
		}
		if got != tt.want {
			t.Errorf("expandPath(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestApplyWorkspace(t *testing.T) {
	src := t.TempDir()
	dst := t.TempDir()

	// Write files to src (simulates what the container produced)
	if err := os.WriteFile(filepath.Join(src, "new.txt"), []byte("new\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(src, "sub"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "sub", "file.txt"), []byte("sub\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	// Write a file to dst that does NOT exist in src — it must NOT be deleted.
	if err := os.WriteFile(filepath.Join(dst, "keep.txt"), []byte("keep\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	if err := applyWorkspace(src, dst, workspaceSkipDirs); err != nil {
		t.Fatalf("applyWorkspace() error = %v", err)
	}

	// new.txt must be promoted
	if _, err := os.Stat(filepath.Join(dst, "new.txt")); err != nil {
		t.Errorf("expected new.txt to be promoted: %v", err)
	}
	// sub/file.txt must be promoted
	if _, err := os.Stat(filepath.Join(dst, "sub", "file.txt")); err != nil {
		t.Errorf("expected sub/file.txt to be promoted: %v", err)
	}
	// keep.txt must NOT be deleted
	if _, err := os.Stat(filepath.Join(dst, "keep.txt")); err != nil {
		t.Errorf("keep.txt should not be deleted: %v", err)
	}
}

func TestRejectUnresolvedGraphFeatures(t *testing.T) {
	err := rejectUnresolvedGraphFeatures(&config.File{Parents: config.RelationSet{Add: []string{"base/locked"}}})
	if err == nil || !strings.Contains(err.Error(), "does not resolve parents") {
		t.Fatalf("error = %v, want parent/child rejection", err)
	}

	err = rejectUnresolvedGraphFeatures(&config.File{Config: config.Config{Runtime: config.RuntimeConfig{PackageOps: config.PackageOpsConfig{Add: []string{"jq"}}}}})
	if err == nil || !strings.Contains(err.Error(), "does not apply") {
		t.Fatalf("error = %v, want packageOps rejection", err)
	}
}
