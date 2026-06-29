package quadlet

import (
	"strings"
	"testing"

	"github.com/zerodawn1990/podman-agent-container/internal/config"
)

func TestRenderRootfsContainerExtendedOptions(t *testing.T) {
	text, err := RenderRootfsContainer(RenderInput{
		Rootfs:                "/tmp/rootfs",
		FallbackContainerName: "name",
		Command:               []string{"/nix/store/runtime/bin/hostname"},
		Config: config.Config{
			Container: config.ContainerConfig{
				Hostname:   "pac-host",
				Entrypoint: []string{"/nix/store/runtime/bin/bash", "-lc"},
				StopSignal: "SIGTERM",
				PodmanArgs: []string{"--log-level=debug"},
			},
			Filesystem: config.FilesystemConfig{
				Mounts:  []string{"type=bind,src=/cache,dst=/cache,ro=true"},
				Devices: []config.DeviceConfig{{Source: "/dev/fuse", Target: "/dev/fuse", Permissions: "rwm"}},
			},
			Network: config.NetworkConfig{
				DNS:     []string{"1.1.1.1"},
				AddHost: []string{"host.containers.internal:host-gateway"},
			},
			Security: config.SecurityConfig{
				Privileged:           boolPtr(false),
				SeccompProfile:       "/etc/seccomp.json",
				SecurityLabelDisable: boolPtr(true),
				SecurityOpt:          []string{"apparmor=unconfined"},
			},
			Resources: config.ResourcesConfig{
				MemorySwap: "2g",
				CPUs:       "2",
				CPUQuota:   "50%",
				Ulimits:    []string{"nofile=1024:2048"},
			},
			Secrets: []config.SecretConfig{{Name: "api-token", Target: "/run/secrets/api-token", Type: "mount", Mode: "0400"}},
			Quadlet: config.QuadletConfig{
				Container: map[string][]string{"Label": {"com.example.test=1"}},
				Service:   map[string][]string{"Environment": {"FROM_SERVICE=1"}},
				Install:   map[string][]string{"WantedBy": {"multi-user.target"}},
			},
		},
	})
	if err != nil {
		t.Fatalf("RenderRootfsContainer() error = %v", err)
	}

	for _, want := range []string{
		"HostName=pac-host",
		"Entrypoint=/nix/store/runtime/bin/bash -lc",
		"StopSignal=SIGTERM",
		"PodmanArgs=--log-level=debug",
		"Mount=type=bind,src=/cache,dst=/cache,ro=true",
		"AddDevice=/dev/fuse:/dev/fuse:rwm",
		"DNS=1.1.1.1",
		"AddHost=host.containers.internal:host-gateway",
		"Privileged=false",
		"SeccompProfile=/etc/seccomp.json",
		"SecurityLabelDisable=true",
		"SecurityOpt=apparmor=unconfined",
		"MemorySwap=2g",
		"CPUs=2",
		"CPUQuota=50%",
		"Ulimit=nofile=1024:2048",
		"Secret=api-token,target=/run/secrets/api-token,type=mount,mode=0400",
		"Label=com.example.test=1",
		"Environment=FROM_SERVICE=1",
		"WantedBy=multi-user.target",
	} {
		if !strings.Contains(text, want) {
			t.Fatalf("rendered Quadlet missing %q\n--- text ---\n%s", want, text)
		}
	}
	if strings.Contains(text, "WantedBy=default.target") {
		t.Fatalf("default WantedBy should not be rendered when install passthrough sets WantedBy\n%s", text)
	}
}
