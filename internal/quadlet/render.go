package quadlet

import (
	"fmt"
	"strings"

	"github.com/zerodawn1990/podman-agent-container/internal/config"
)

type RenderInput struct {
	Rootfs                string
	FallbackContainerName string
	Command               []string
	Config                config.Config
}

func RenderRootfsContainer(input RenderInput) (string, error) {
	if input.Rootfs == "" {
		return "", fmt.Errorf("rootfs is required")
	}
	if input.FallbackContainerName == "" {
		return "", fmt.Errorf("fallback container name is required")
	}
	if len(input.Command) == 0 {
		return "", fmt.Errorf("command is required")
	}

	containerName := input.FallbackContainerName
	if input.Config.Container.Name != "" {
		containerName = input.Config.Container.Name
	}

	var b strings.Builder
	b.WriteString("[Unit]\n")
	b.WriteString("Description=podman-agent-container rootfs run\n\n")
	b.WriteString("[Container]\n")
	b.WriteString("Rootfs=" + input.Rootfs + "\n")
	b.WriteString("ContainerName=" + containerName + "\n")
	b.WriteString("AutoUpdate=none\n")
	if !hasVolumeTarget(input.Config.Filesystem.Volumes, "/nix/store") {
		b.WriteString("Volume=/nix/store:/nix/store:ro\n")
	}
	b.WriteString("Exec=" + quoteExec(input.Command) + "\n")

	writeStringOption(&b, "WorkingDir", input.Config.Container.WorkingDir)
	writeStringOption(&b, "User", input.Config.Container.User)
	writeStringOption(&b, "Group", input.Config.Container.Group)
	for _, key := range sortedKeys(input.Config.Container.Environment) {
		b.WriteString("Environment=" + SystemdQuote(key+"="+input.Config.Container.Environment[key]) + "\n")
	}

	writeBoolOption(&b, "ReadOnly", input.Config.Filesystem.ReadOnly)
	writeBoolOption(&b, "ReadOnlyTmpfs", input.Config.Filesystem.ReadOnlyTmpfs)
	for _, tmpfs := range input.Config.Filesystem.Tmpfs {
		b.WriteString("Tmpfs=" + tmpfs + "\n")
	}
	for _, volume := range input.Config.Filesystem.Volumes {
		if volume.Source == "" || volume.Target == "" {
			continue
		}
		value := volume.Source + ":" + volume.Target
		if volume.Mode != "" {
			value += ":" + volume.Mode
		}
		b.WriteString("Volume=" + value + "\n")
	}

	writeStringOption(&b, "Network", input.Config.Network.Mode)
	for _, publish := range input.Config.Network.Publish {
		b.WriteString("PublishPort=" + publish + "\n")
	}

	for _, cap := range input.Config.Security.DropCapabilities {
		b.WriteString("DropCapability=" + cap + "\n")
	}
	for _, cap := range input.Config.Security.AddCapabilities {
		b.WriteString("AddCapability=" + cap + "\n")
	}
	writeBoolOption(&b, "NoNewPrivileges", input.Config.Security.NoNewPrivileges)
	writeStringOption(&b, "UserNS", input.Config.Security.UserNS)

	writeStringOption(&b, "Memory", input.Config.Resources.Memory)
	if input.Config.Resources.PidsLimit != 0 {
		_, _ = fmt.Fprintf(&b, "PidsLimit=%d\n", input.Config.Resources.PidsLimit)
	}

	b.WriteString("\n[Service]\n")
	b.WriteString("Type=oneshot\n")
	b.WriteString("RemainAfterExit=no\n\n")
	b.WriteString("[Install]\n")
	b.WriteString("WantedBy=default.target\n")
	return b.String(), nil
}

func hasVolumeTarget(volumes []config.VolumeConfig, target string) bool {
	for _, volume := range volumes {
		if volume.Target == target {
			return true
		}
	}
	return false
}

func writeStringOption(b *strings.Builder, key, value string) {
	if value != "" {
		b.WriteString(key + "=" + value + "\n")
	}
}

func writeBoolOption(b *strings.Builder, key string, value *bool) {
	if value != nil {
		_, _ = fmt.Fprintf(b, "%s=%t\n", key, *value)
	}
}
