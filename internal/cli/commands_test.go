package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/zerodawn1990/podman-agent-container/internal/config"
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
	for _, name := range []string{"config.toml", ".pac.toml", "podman-agent-container.toml", "pac.toml"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("version = 1\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	path, err := autodetectConfig()
	if err != nil {
		t.Fatalf("autodetectConfig() error = %v", err)
	}
	if path != "pac.toml" {
		t.Fatalf("path = %q, want pac.toml", path)
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
	got := nixString(`a"b${system}`)
	want := `"a\\"b\${system}"`
	if got != want {
		t.Fatalf("nixString() = %q, want %q", got, want)
	}
}
