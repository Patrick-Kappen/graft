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

[config.service]
type = "simple"
restart = "on-failure"
restartSec = "10s"
timeoutStartSec = "2m"
timeoutStopSec = "30s"
remainAfterExit = false
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
	if file.Config.Service.Type != "simple" {
		t.Fatalf("Service.Type = %q, want simple", file.Config.Service.Type)
	}
	if file.Config.Service.RemainAfterExit == nil || *file.Config.Service.RemainAfterExit {
		t.Fatal("expected service.remainAfterExit=false")
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

func TestLoadPackageOps(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "package-ops"

[config.runtime.packageOps]
remove = ["old"]
add = ["new"]

[[config.runtime.packageOps.replace]]
name = "tool"
with = "toolPinned"
`)

	file, err := Load(path)
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	ops := file.Config.Runtime.PackageOps
	if len(ops.Remove) != 1 || ops.Remove[0] != "old" {
		t.Fatalf("Remove = %#v", ops.Remove)
	}
	if len(ops.Add) != 1 || ops.Add[0] != "new" {
		t.Fatalf("Add = %#v", ops.Add)
	}
	if len(ops.Replace) != 1 || ops.Replace[0].Name != "tool" || ops.Replace[0].With != "toolPinned" {
		t.Fatalf("Replace = %#v", ops.Replace)
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

func TestLoadRejectsDuplicateVolumeTargets(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[[config.filesystem.volumes]]
source = "/a"
target = "/data"
mode = "ro"

[[config.filesystem.volumes]]
source = "/b"
target = "/data"
mode = "ro"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), `duplicate filesystem volume target "/data"`) {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsIncompleteVolume(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[[config.filesystem.volumes]]
target = "/data"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "filesystem volume must set both source and target") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsInvalidDevice(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[[config.filesystem.devices]]
target = "/dev/fuse"
permissions = "rwm"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "filesystem device must set source") {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsDuplicateSecretNames(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[[config.secrets]]
name = "token"

[[config.secrets]]
name = "token"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), `duplicate secret name "token"`) {
		t.Fatalf("error = %q", err)
	}
}

func TestLoadRejectsUnnamedSecret(t *testing.T) {
	path := writeTempConfig(t, `
version = 1
name = "bad"

[[config.secrets]]
target = "/run/secrets/token"
`)

	_, err := Load(path)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "secret must set name") {
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
