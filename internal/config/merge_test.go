package config_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/Patrick-Kappen/graft/internal/config"
)

// --------------------------------------------------------------------------
// ApplyPackageOps
// --------------------------------------------------------------------------

func TestApplyPackageOps_Add(t *testing.T) {
	pkgs := config.ApplyPackageOps([]string{"a", "b"}, config.PackageOpsConfig{Add: []string{"c", "a"}})
	want := []string{"a", "b", "c"}
	assertStringSlice(t, pkgs, want)
}

func TestApplyPackageOps_Remove(t *testing.T) {
	pkgs := config.ApplyPackageOps([]string{"a", "b", "c"}, config.PackageOpsConfig{Remove: []string{"b"}})
	want := []string{"a", "c"}
	assertStringSlice(t, pkgs, want)
}

func TestApplyPackageOps_Replace(t *testing.T) {
	pkgs := config.ApplyPackageOps(
		[]string{"a", "hello", "c"},
		config.PackageOpsConfig{Replace: []config.PackageReplaceConfig{{Name: "hello", With: "hostname"}}},
	)
	want := []string{"a", "hostname", "c"}
	assertStringSlice(t, pkgs, want)
}

func TestApplyPackageOps_Combined(t *testing.T) {
	pkgs := config.ApplyPackageOps(
		[]string{"coreutils", "hello", "bash"},
		config.PackageOpsConfig{
			Add:     []string{"ripgrep"},
			Remove:  []string{"bash"},
			Replace: []config.PackageReplaceConfig{{Name: "hello", With: "hostname"}},
		},
	)
	want := []string{"coreutils", "hostname", "ripgrep"}
	assertStringSlice(t, pkgs, want)
}

// --------------------------------------------------------------------------
// MergeFiles — scalar inheritance
// --------------------------------------------------------------------------

func TestMergeFiles_ChildWinsNonEmpty(t *testing.T) {
	base := makeFile("base", func(f *config.File) {
		f.Config.Service.Type = "notify"
		f.Config.Service.Restart = "on-failure"
		f.Config.Runtime.Mode = "rootfs-store"
	})
	child := makeFile("child", func(f *config.File) {
		f.Config.Service.Restart = "always"
	})
	out := config.MergeFiles(base, child)

	if out.Config.Service.Type != "notify" {
		t.Errorf("expected Type=notify (inherited), got %q", out.Config.Service.Type)
	}
	if out.Config.Service.Restart != "always" {
		t.Errorf("expected Restart=always (child wins), got %q", out.Config.Service.Restart)
	}
	if out.Config.Runtime.Mode != "rootfs-store" {
		t.Errorf("expected Mode=rootfs-store (inherited), got %q", out.Config.Runtime.Mode)
	}
}

func TestMergeFiles_EnvironmentUnion(t *testing.T) {
	base := makeFile("base", func(f *config.File) {
		f.Config.Container.Environment = map[string]string{"TERM": "xterm-256color", "FOO": "base"}
	})
	child := makeFile("child", func(f *config.File) {
		f.Config.Container.Environment = map[string]string{"FOO": "child", "BAR": "1"}
	})
	out := config.MergeFiles(base, child)
	env := out.Config.Container.Environment
	if env["TERM"] != "xterm-256color" {
		t.Errorf("TERM: expected xterm-256color, got %q", env["TERM"])
	}
	if env["FOO"] != "child" {
		t.Errorf("FOO: expected child (override), got %q", env["FOO"])
	}
	if env["BAR"] != "1" {
		t.Errorf("BAR: expected 1, got %q", env["BAR"])
	}
}

func TestMergeFiles_PackageUnion(t *testing.T) {
	base := makeFile("base", func(f *config.File) {
		f.Config.Runtime.Packages = []string{"tmux", "coreutils"}
	})
	child := makeFile("child", func(f *config.File) {
		f.Config.Runtime.Packages = []string{"ripgrep", "tmux"}
	})
	out := config.MergeFiles(base, child)
	want := []string{"tmux", "coreutils", "ripgrep"}
	assertStringSlice(t, out.Config.Runtime.Packages, want)
}

func TestMergeFiles_VolumesMergeByTarget(t *testing.T) {
	base := makeFile("base", func(f *config.File) {
		f.Config.Filesystem.Volumes = []config.VolumeConfig{
			{Source: "/nix/store", Target: "/nix/store", Mode: "ro"},
			{Source: "/base/data", Target: "/data", Mode: "ro"},
		}
	})
	child := makeFile("child", func(f *config.File) {
		f.Config.Filesystem.Volumes = []config.VolumeConfig{
			{Source: "/child/data", Target: "/data", Mode: "rw"}, // same target → override
			{Source: "/workspace", Target: "/workspace", Mode: "rw"},
		}
	})
	out := config.MergeFiles(base, child)

	byTarget := make(map[string]config.VolumeConfig)
	for _, v := range out.Config.Filesystem.Volumes {
		byTarget[v.Target] = v
	}

	if v := byTarget["/nix/store"]; v.Source != "/nix/store" {
		t.Errorf("/nix/store: expected source /nix/store, got %q", v.Source)
	}
	if v := byTarget["/data"]; v.Source != "/child/data" || v.Mode != "rw" {
		t.Errorf("/data: expected child override, got %+v", v)
	}
	if _, ok := byTarget["/workspace"]; !ok {
		t.Error("/workspace: missing child volume")
	}
}

func TestMergeFiles_ClearsParents(t *testing.T) {
	base := makeFile("base", nil)
	child := makeFile("child", func(f *config.File) {
		f.Parents.Add = []string{"base"}
	})
	out := config.MergeFiles(base, child)
	if len(out.Parents.Add) != 0 || len(out.Parents.Set) != 0 {
		t.Error("parents should be cleared after merge")
	}
}

// --------------------------------------------------------------------------
// LoadResolved
// --------------------------------------------------------------------------

func TestLoadResolved_NoParents(t *testing.T) {
	dir := t.TempDir()
	writeConfig(t, filepath.Join(dir, "simple.toml"), `
version = 1
name = "simple"
[config.runtime]
mode = "rootfs-store"
command = ["bash"]
packages = ["bashInteractive"]
`)
	file, err := config.LoadResolved(filepath.Join(dir, "simple.toml"), nil)
	if err != nil {
		t.Fatal(err)
	}
	if file.Name != "simple" {
		t.Errorf("name: got %q", file.Name)
	}
	if len(file.Config.Runtime.Packages) != 1 || file.Config.Runtime.Packages[0] != "bashInteractive" {
		t.Errorf("packages: %v", file.Config.Runtime.Packages)
	}
}

func TestLoadResolved_SingleParent(t *testing.T) {
	dir := t.TempDir()
	writeConfig(t, filepath.Join(dir, "base.toml"), `
version = 1
name = "base"
[config.service]
type = "notify"
restart = "on-failure"
[config.runtime]
mode = "rootfs-store"
packages = ["tmux"]
[config.container.environment]
TERM = "xterm-256color"
[[config.filesystem.volumes]]
source = "/nix/store"
target = "/nix/store"
mode = "ro"
`)
	writeConfig(t, filepath.Join(dir, "pi.toml"), `
version = 1
name = "pi"
[parents]
add = ["base"]
[config.runtime]
command = ["tmux", "new-session", "-A", "-s", "main", "pi-agent"]
packages = ["pi-agent"]
`)
	file, err := config.LoadResolved(filepath.Join(dir, "pi.toml"), nil)
	if err != nil {
		t.Fatal(err)
	}
	// Child name wins.
	if file.Name != "pi" {
		t.Errorf("name: got %q", file.Name)
	}
	// Service inherited from base.
	if file.Config.Service.Type != "notify" {
		t.Errorf("service.type: got %q", file.Config.Service.Type)
	}
	// Packages: base (tmux) + child (pi-agent).
	wantPkgs := []string{"tmux", "pi-agent"}
	assertStringSlice(t, file.Config.Runtime.Packages, wantPkgs)
	// Volume inherited from base.
	if len(file.Config.Filesystem.Volumes) == 0 || file.Config.Filesystem.Volumes[0].Target != "/nix/store" {
		t.Errorf("volumes: %v", file.Config.Filesystem.Volumes)
	}
	// Environment inherited.
	if file.Config.Container.Environment["TERM"] != "xterm-256color" {
		t.Errorf("env TERM: %q", file.Config.Container.Environment["TERM"])
	}
	// Parents cleared.
	if len(file.Parents.Add) != 0 {
		t.Error("parents should be cleared")
	}
}

func TestLoadResolved_PackageOpsApplied(t *testing.T) {
	dir := t.TempDir()
	writeConfig(t, filepath.Join(dir, "base.toml"), `
version = 1
name = "base"
[config.runtime]
mode = "rootfs-store"
packages = ["coreutils", "hello", "bash"]
`)
	writeConfig(t, filepath.Join(dir, "child.toml"), `
version = 1
name = "child"
[parents]
add = ["base"]
[config.runtime]
[config.runtime.packageOps]
add = ["ripgrep"]
remove = ["bash"]
[[config.runtime.packageOps.replace]]
name = "hello"
with = "hostname"
`)
	file, err := config.LoadResolved(filepath.Join(dir, "child.toml"), nil)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"coreutils", "hostname", "ripgrep"}
	assertStringSlice(t, file.Config.Runtime.Packages, want)
}

func TestLoadResolved_CycleDetected(t *testing.T) {
	dir := t.TempDir()
	writeConfig(t, filepath.Join(dir, "a.toml"), `
version = 1
name = "a"
[parents]
add = ["b"]
[config.runtime]
mode = "rootfs-store"
`)
	writeConfig(t, filepath.Join(dir, "b.toml"), `
version = 1
name = "b"
[parents]
add = ["a"]
[config.runtime]
mode = "rootfs-store"
`)
	_, err := config.LoadResolved(filepath.Join(dir, "a.toml"), nil)
	if err == nil {
		t.Fatal("expected cycle error, got nil")
	}
}

func TestLoadResolved_SearchDirs(t *testing.T) {
	base := t.TempDir()
	repo := t.TempDir()

	// base.toml lives in base dir (config root)
	writeConfig(t, filepath.Join(base, "base.toml"), `
version = 1
name = "base"
[config.service]
type = "notify"
[config.runtime]
mode = "rootfs-store"
packages = ["tmux"]
`)
	// child.toml lives in repo dir (local)
	writeConfig(t, filepath.Join(repo, "child.toml"), `
version = 1
name = "child"
[parents]
add = ["base"]
[config.runtime]
command = ["bash"]
`)
	file, err := config.LoadResolved(filepath.Join(repo, "child.toml"), []string{base})
	if err != nil {
		t.Fatal(err)
	}
	if file.Config.Service.Type != "notify" {
		t.Errorf("service.type inherited from base: got %q", file.Config.Service.Type)
	}
}

// --------------------------------------------------------------------------
// helpers
// --------------------------------------------------------------------------

func makeFile(name string, fn func(*config.File)) *config.File {
	f := &config.File{Version: 1, Name: name}
	if fn != nil {
		fn(f)
	}
	return f
}

func writeConfig(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

func assertStringSlice(t *testing.T, got, want []string) {
	t.Helper()
	if len(got) != len(want) {
		t.Errorf("slice length: got %d (%v), want %d (%v)", len(got), got, len(want), want)
		return
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("[%d]: got %q, want %q", i, got[i], want[i])
		}
	}
}
