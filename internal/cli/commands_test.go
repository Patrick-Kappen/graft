package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/zerodawn1990/graft/internal/config"
)

func TestGraftRunRequiresAsFlag(t *testing.T) {
	err := graftRun([]string{"some.toml"})
	if err == nil || !strings.Contains(err.Error(), "--as") {
		t.Fatalf("expected --as error, got %v", err)
	}
}

func TestGraftRunRequiresTomlFile(t *testing.T) {
	err := graftRun([]string{"--as", "my-instance"})
	if err == nil || !strings.Contains(err.Error(), "<file.toml>") {
		t.Fatalf("expected file.toml error, got %v", err)
	}
}

func TestGraftRunRejectsUnexpectedArg(t *testing.T) {
	err := graftRun([]string{"a.toml", "b.toml", "--as", "x"})
	if err == nil || !strings.Contains(err.Error(), "unexpected argument") {
		t.Fatalf("expected unexpected argument error, got %v", err)
	}
}

func TestGraftRunAsRequiresValue(t *testing.T) {
	err := graftRun([]string{"a.toml", "--as"})
	if err == nil || !strings.Contains(err.Error(), "--as requires an argument") {
		t.Fatalf("expected --as value error, got %v", err)
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

func TestGraftStartOrAttachRejectsPathSeparator(t *testing.T) {
	cases := []string{
		"../../etc/passwd",
		"sub/name",
		".",
	}
	for _, name := range cases {
		err := graftStartOrAttach(name, nil, "")
		if err == nil || !strings.Contains(err.Error(), "path separators") {
			t.Errorf("graftStartOrAttach(%q): expected path separator error, got %v", name, err)
		}
	}
}

func TestWithRuntimePathEnv(t *testing.T) {
	cfg := config.Config{}

	// No-op when args is empty.
	out := withRuntimePathEnv(cfg, nil)
	if out.Container.Environment != nil {
		t.Error("expected nil env when args empty")
	}
	// No-op when first arg is not a /nix/store path.
	out = withRuntimePathEnv(cfg, []string{"bash"})
	if out.Container.Environment != nil {
		t.Error("expected nil env for non-store path")
	}
	// Sets PATH derived from the store binary.
	out = withRuntimePathEnv(cfg, []string{"/nix/store/abc-bash/bin/bash"})
	if got := out.Container.Environment["PATH"]; got != "/nix/store/abc-bash/bin" {
		t.Errorf("PATH = %q, want /nix/store/abc-bash/bin", got)
	}
	// Does not overwrite an existing PATH.
	cfg.Container.Environment = map[string]string{"PATH": "/custom"}
	out = withRuntimePathEnv(cfg, []string{"/nix/store/abc-bash/bin/bash"})
	if got := out.Container.Environment["PATH"]; got != "/custom" {
		t.Errorf("PATH = %q, want /custom (existing should not be overwritten)", got)
	}
}

func TestNixSystem(t *testing.T) {
	sys, err := nixSystem()
	if err != nil {
		t.Fatalf("nixSystem() error = %v", err)
	}
	allowed := map[string]bool{
		"x86_64-linux": true, "aarch64-linux": true,
		"x86_64-darwin": true, "aarch64-darwin": true,
	}
	if !allowed[sys] {
		t.Errorf("nixSystem() = %q, not a recognised Nix system string", sys)
	}
}

func TestShouldSkipDir(t *testing.T) {
	dirs := []string{".git", ".jj", "node_modules"}
	for _, skip := range dirs {
		if !shouldSkipDir(skip, dirs) {
			t.Errorf("shouldSkipDir(%q) = false, want true", skip)
		}
	}
	for _, keep := range []string{"src", "lib", "cmd"} {
		if shouldSkipDir(keep, dirs) {
			t.Errorf("shouldSkipDir(%q) = true, want false", keep)
		}
	}
}

func TestShadowDirName(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"/workspace", "workspace"},
		{"/home/user/data", "home-user-data"},
		{"/", "root"},
		{"", "root"},
		{"/a/b/c", "a-b-c"},
	}
	for _, tt := range tests {
		if got := shadowDirName(tt.input); got != tt.want {
			t.Errorf("shadowDirName(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestSessionMetaRoundtrip(t *testing.T) {
	// Override XDG_DATA_HOME so sessionMetaPath resolves to a temp dir.
	tmpDir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", tmpDir)

	name := "test-container"
	meta := sessionMeta{
		HomeSource:  "/home/user/src",
		HomeSession: "/tmp/session/home",
		HomeReview:  "diff",
		HomePromote: "prompt",
		Shadows: []shadowMeta{
			{ContainerPath: "/workspace", HostPath: "/home/user/project", SessionDir: "/tmp/shadow"},
		},
	}
	if err := writeSessionMeta(name, meta); err != nil {
		t.Fatalf("writeSessionMeta: %v", err)
	}
	got, ok := readSessionMeta(name)
	if !ok {
		t.Fatal("readSessionMeta returned false")
	}
	if got.HomeSource != meta.HomeSource || got.HomePromote != meta.HomePromote {
		t.Errorf("meta mismatch: got %+v, want %+v", got, meta)
	}
	if len(got.Shadows) != 1 || got.Shadows[0].ContainerPath != "/workspace" {
		t.Errorf("shadow mismatch: got %+v", got.Shadows)
	}
}

func TestGraftDiffNoArgs(t *testing.T) {
	err := graftDiff(nil, "")
	if err == nil || !strings.Contains(err.Error(), "usage") {
		t.Fatalf("expected usage error, got %v", err)
	}
}

func TestGraftDiffNoMeta(t *testing.T) {
	tmpDir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", tmpDir)

	err := graftDiff([]string{"nonexistent-container"}, "")
	if err == nil || !strings.Contains(err.Error(), "no session data") {
		t.Fatalf("expected no session data error, got %v", err)
	}
}

func TestGraftPromoteNoArgs(t *testing.T) {
	err := graftPromote(nil, "")
	if err == nil || !strings.Contains(err.Error(), "usage") {
		t.Fatalf("expected usage error, got %v", err)
	}
}

func TestGraftPromoteNoMeta(t *testing.T) {
	tmpDir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", tmpDir)

	err := graftPromote([]string{"nonexistent-container"}, "")
	if err == nil || !strings.Contains(err.Error(), "no session data") {
		t.Fatalf("expected no session data error, got %v", err)
	}
}

func TestGraftResetNoArgs(t *testing.T) {
	err := graftReset(nil, "")
	if err == nil || !strings.Contains(err.Error(), "usage") {
		t.Fatalf("expected usage error, got %v", err)
	}
}

func TestGraftResetNoData(t *testing.T) {
	tmpDir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", tmpDir)

	// Should be a no-op (no error) when session dir doesn't exist.
	if err := graftReset([]string{"nonexistent-container"}, ""); err != nil {
		t.Fatalf("graftReset with no session data: %v", err)
	}
}

func TestGraftPromoteShadows(t *testing.T) {
	tmpDir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", tmpDir)

	// Set up a session dir with a file.
	sessionDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(sessionDir, "output.txt"), []byte("result\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	hostDir := t.TempDir()

	name := "promote-test"
	if err := writeSessionMeta(name, sessionMeta{
		Shadows: []shadowMeta{
			{ContainerPath: "/workspace", HostPath: hostDir, SessionDir: sessionDir},
		},
	}); err != nil {
		t.Fatal(err)
	}

	if err := graftPromote([]string{name}, ""); err != nil {
		t.Fatalf("graftPromote: %v", err)
	}
	if _, err := os.Stat(filepath.Join(hostDir, "output.txt")); err != nil {
		t.Errorf("output.txt not promoted: %v", err)
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
