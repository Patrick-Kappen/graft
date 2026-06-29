package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func writeTempConfig(t *testing.T, content string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "config.toml")
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestLoadNoopConfig(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "noop"

[config]
`)

	file, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if !file.IsNoop() {
		t.Fatal("expected no-op config")
	}
	if file.Name != "noop" {
		t.Fatalf("Name = %q, want noop", file.Name)
	}
}

func TestLoadRuntimeConfig(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "example"

[deploy]
enable = true
target = "system"

[config.runtime]
mode = "rootfs-store"
packages = ["bashInteractive", "coreutils"]
command = ["bash", "-lc", "echo hello"]

[config.container.environment]
FOO = "bar"
`)

	file, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if file.IsNoop() {
		t.Fatal("expected active config")
	}
	if !file.Deploy.Enable {
		t.Fatal("expected deploy.enable")
	}
	if file.Deploy.Target != "system" {
		t.Fatalf("Deploy.Target = %q, want system", file.Deploy.Target)
	}
	if got := file.Config.Runtime.Packages; len(got) != 2 || got[0] != "bashInteractive" || got[1] != "coreutils" {
		t.Fatalf("Packages = %#v", got)
	}
	if file.Config.Container.Environment["FOO"] != "bar" {
		t.Fatalf("Environment[FOO] = %q", file.Config.Container.Environment["FOO"])
	}
}

func TestLoadRejectsUnknownField(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"
unknown = true

[config]
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "unknown TOML field unknown") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsUnsupportedRuntimeMode(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[config.runtime]
mode = "oci"
command = ["bash"]
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "unsupported runtime mode") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadAllowsRuntimePackageFragment(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "runtime-fragment"

[config.runtime]
packages = ["bashInteractive"]
`)

	file, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if file.IsNoop() {
		t.Fatal("expected package fragment not to be no-op")
	}
}

func TestLoadAllowsSecurityOnlyFragment(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "locked"

[config.security]
dropCapabilities = ["all"]
noNewPrivileges = true
`)

	file, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if file.IsNoop() {
		t.Fatal("expected security fragment not to be no-op")
	}
	if file.Config.Security.NoNewPrivileges == nil || !*file.Config.Security.NoNewPrivileges {
		t.Fatal("expected noNewPrivileges=true")
	}
}

func TestLoadRejectsUnknownNestedField(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[config.runtime]
mode = "rootfs-store"
command = ["bash"]
unknownRuntimeField = true
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "unknown TOML field config.runtime.unknownRuntimeField") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsInvalidVersion(t *testing.T) {
	path := writeTempConfig(t, `
version = 2
name = "bad"

[config]
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "unsupported or missing version 2") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsInvalidType(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[config.runtime]
packages = "bashInteractive"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "incompatible types") {
		t.Fatalf("error = %q", err)
	}
}
