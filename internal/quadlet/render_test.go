package quadlet

import (
	"strings"
	"testing"

	"github.com/zerodawn1990/podman-agent-container/internal/config"
)

func boolPtr(value bool) *bool { return &value }

func TestRenderRootfsContainer(t *testing.T) {
	text, err := RenderRootfsContainer(RenderInput{
		Rootfs:                "/tmp/rootfs",
		FallbackContainerName: "fallback-name",
		Command:               []string{"/nix/store/runtime/bin/bash", "-lc", "echo hello"},
		Config: config.Config{
			Container: config.ContainerConfig{
				Name:       "configured-name",
				WorkingDir: "/workspace",
				Environment: map[string]string{
					"FOO": "bar baz",
				},
			},
			Filesystem: config.FilesystemConfig{
				ReadOnly:      boolPtr(true),
				ReadOnlyTmpfs: boolPtr(true),
				Tmpfs:         []string{"/tmp"},
				Volumes: []config.VolumeConfig{
					{Source: "/data", Target: "/data", Mode: "ro"},
				},
			},
			Network: config.NetworkConfig{Mode: "none"},
			Security: config.SecurityConfig{
				DropCapabilities: []string{"all"},
				NoNewPrivileges:  boolPtr(true),
				UserNS:           "keep-id",
			},
			Resources: config.ResourcesConfig{Memory: "1g", PidsLimit: 128},
			Service: config.ServiceConfig{
				Type:            "simple",
				Restart:         "on-failure",
				RestartSec:      "10s",
				TimeoutStartSec: "2m",
				TimeoutStopSec:  "30s",
				RemainAfterExit: boolPtr(false),
			},
		},
	})
	if err != nil {
		t.Fatalf("RenderRootfsContainer() error = %v", err)
	}

	for _, want := range []string{
		"Rootfs=/tmp/rootfs",
		"ContainerName=configured-name",
		"AutoUpdate=none",
		"Volume=/nix/store:/nix/store:ro",
		"Volume=/data:/data:ro",
		"Exec=/nix/store/runtime/bin/bash -lc \"echo hello\"",
		"WorkingDir=/workspace",
		"Environment=\"FOO=bar baz\"",
		"ReadOnly=true",
		"ReadOnlyTmpfs=true",
		"Tmpfs=/tmp",
		"Network=none",
		"DropCapability=all",
		"NoNewPrivileges=true",
		"UserNS=keep-id",
		"Memory=1g",
		"PidsLimit=128",
		"Type=simple",
		"RemainAfterExit=false",
		"Restart=on-failure",
		"RestartSec=10s",
		"TimeoutStartSec=2m",
		"TimeoutStopSec=30s",
		"WantedBy=default.target",
	} {
		if !strings.Contains(text, want) {
			t.Fatalf("rendered Quadlet missing %q\n--- text ---\n%s", want, text)
		}
	}
}

func TestRenderRootfsContainerUsesFallbackName(t *testing.T) {
	text, err := RenderRootfsContainer(RenderInput{
		Rootfs:                "/tmp/rootfs",
		FallbackContainerName: "fallback-name",
		Command:               []string{"/nix/store/runtime/bin/bash"},
	})
	if err != nil {
		t.Fatalf("RenderRootfsContainer() error = %v", err)
	}
	if !strings.Contains(text, "ContainerName=fallback-name") {
		t.Fatalf("missing fallback container name\n%s", text)
	}
}

func TestSystemdQuote(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{input: "plain", want: "plain"},
		{input: "", want: `""`},
		{input: "hello world", want: `"hello world"`},
		{input: `a"b`, want: `"a\"b"`},
		{input: "semi;hash#", want: `"semi;hash#"`},
	}
	for _, test := range tests {
		if got := SystemdQuote(test.input); got != test.want {
			t.Fatalf("SystemdQuote(%q) = %q, want %q", test.input, got, test.want)
		}
	}
}

func TestRenderRootfsContainerDoesNotDuplicateNixStoreVolume(t *testing.T) {
	text, err := RenderRootfsContainer(RenderInput{
		Rootfs:                "/tmp/rootfs",
		FallbackContainerName: "name",
		Command:               []string{"/nix/store/runtime/bin/bash"},
		Config: config.Config{
			Filesystem: config.FilesystemConfig{
				Volumes: []config.VolumeConfig{{Source: "/custom/store", Target: "/nix/store", Mode: "ro"}},
			},
		},
	})
	if err != nil {
		t.Fatalf("RenderRootfsContainer() error = %v", err)
	}
	if strings.Contains(text, "Volume=/nix/store:/nix/store:ro") {
		t.Fatalf("unexpected automatic /nix/store volume when target already exists\n%s", text)
	}
	if !strings.Contains(text, "Volume=/custom/store:/nix/store:ro") {
		t.Fatalf("missing custom /nix/store volume\n%s", text)
	}
}

func TestRenderRootfsContainerRequiresFields(t *testing.T) {
	if _, err := RenderRootfsContainer(RenderInput{}); err == nil {
		t.Fatal("expected missing rootfs error")
	}
	if _, err := RenderRootfsContainer(RenderInput{Rootfs: "/tmp/rootfs"}); err == nil {
		t.Fatal("expected missing fallback name error")
	}
	if _, err := RenderRootfsContainer(RenderInput{Rootfs: "/tmp/rootfs", FallbackContainerName: "name"}); err == nil {
		t.Fatal("expected missing command error")
	}
}
