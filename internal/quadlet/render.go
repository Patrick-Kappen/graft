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

	writeStringOption(&b, "HostName", input.Config.Container.Hostname)
	if len(input.Config.Container.Entrypoint) > 0 {
		b.WriteString("Entrypoint=" + quoteExec(input.Config.Container.Entrypoint) + "\n")
	}
	writeStringOption(&b, "StopSignal", input.Config.Container.StopSignal)
	writeStringOption(&b, "WorkingDir", input.Config.Container.WorkingDir)
	writeStringOption(&b, "User", input.Config.Container.User)
	writeStringOption(&b, "Group", input.Config.Container.Group)
	for _, key := range sortedKeys(input.Config.Container.Environment) {
		b.WriteString("Environment=" + SystemdQuote(key+"="+input.Config.Container.Environment[key]) + "\n")
	}
	for _, arg := range input.Config.Container.PodmanArgs {
		b.WriteString("PodmanArgs=" + SystemdQuote(arg) + "\n")
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
	for _, mount := range input.Config.Filesystem.Mounts {
		b.WriteString("Mount=" + mount + "\n")
	}
	for _, device := range input.Config.Filesystem.Devices {
		if device.Source == "" {
			continue
		}
		value := device.Source
		if device.Target != "" {
			value += ":" + device.Target
			if device.Permissions != "" {
				value += ":" + device.Permissions
			}
		} else if device.Permissions != "" {
			value += ":" + device.Permissions
		}
		b.WriteString("AddDevice=" + value + "\n")
	}

	writeStringOption(&b, "Network", input.Config.Network.Mode)
	for _, publish := range input.Config.Network.Publish {
		b.WriteString("PublishPort=" + publish + "\n")
	}
	for _, dns := range input.Config.Network.DNS {
		b.WriteString("DNS=" + dns + "\n")
	}
	for _, host := range input.Config.Network.AddHost {
		b.WriteString("AddHost=" + host + "\n")
	}

	for _, cap := range input.Config.Security.DropCapabilities {
		b.WriteString("DropCapability=" + cap + "\n")
	}
	for _, cap := range input.Config.Security.AddCapabilities {
		b.WriteString("AddCapability=" + cap + "\n")
	}
	writeBoolOption(&b, "NoNewPrivileges", input.Config.Security.NoNewPrivileges)
	writeBoolOption(&b, "Privileged", input.Config.Security.Privileged)
	writeStringOption(&b, "SeccompProfile", input.Config.Security.SeccompProfile)
	writeBoolOption(&b, "SecurityLabelDisable", input.Config.Security.SecurityLabelDisable)
	for _, opt := range input.Config.Security.SecurityOpt {
		b.WriteString("SecurityOpt=" + opt + "\n")
	}
	writeStringOption(&b, "UserNS", input.Config.Security.UserNS)

	writeStringOption(&b, "Memory", input.Config.Resources.Memory)
	writeStringOption(&b, "MemorySwap", input.Config.Resources.MemorySwap)
	writeStringOption(&b, "CPUs", input.Config.Resources.CPUs)
	writeStringOption(&b, "CPUQuota", input.Config.Resources.CPUQuota)
	if input.Config.Resources.PidsLimit != 0 {
		_, _ = fmt.Fprintf(&b, "PidsLimit=%d\n", input.Config.Resources.PidsLimit)
	}
	for _, ulimit := range input.Config.Resources.Ulimits {
		b.WriteString("Ulimit=" + ulimit + "\n")
	}
	for _, secret := range input.Config.Secrets {
		if secret.Name == "" {
			continue
		}
		b.WriteString("Secret=" + renderSecret(secret) + "\n")
	}
	writePassthroughOptions(&b, input.Config.Quadlet.Container)

	b.WriteString("\n[Service]\n")
	serviceType := input.Config.Service.Type
	if serviceType == "" {
		serviceType = "oneshot"
	}
	b.WriteString("Type=" + serviceType + "\n")
	if input.Config.Service.RemainAfterExit != nil {
		writeBoolOption(&b, "RemainAfterExit", input.Config.Service.RemainAfterExit)
	} else {
		b.WriteString("RemainAfterExit=no\n")
	}
	writeStringOption(&b, "Restart", input.Config.Service.Restart)
	writeStringOption(&b, "RestartSec", input.Config.Service.RestartSec)
	writeStringOption(&b, "TimeoutStartSec", input.Config.Service.TimeoutStartSec)
	writeStringOption(&b, "TimeoutStopSec", input.Config.Service.TimeoutStopSec)
	writePassthroughOptions(&b, input.Config.Quadlet.Service)
	b.WriteString("\n[Install]\n")
	if _, ok := input.Config.Quadlet.Install["WantedBy"]; !ok {
		b.WriteString("WantedBy=default.target\n")
	}
	writePassthroughOptions(&b, input.Config.Quadlet.Install)
	return b.String(), nil
}

func renderSecret(secret config.SecretConfig) string {
	value := secret.Name
	appendPart := func(key, part string) {
		if part != "" {
			value += "," + key + "=" + part
		}
	}
	appendPart("target", secret.Target)
	appendPart("type", secret.Type)
	appendPart("uid", secret.UID)
	appendPart("gid", secret.GID)
	appendPart("mode", secret.Mode)
	if secret.Options != "" {
		value += "," + secret.Options
	}
	return value
}

func writePassthroughOptions(b *strings.Builder, values map[string][]string) {
	for _, key := range sortedOptionKeys(values) {
		for _, value := range values[key] {
			b.WriteString(key + "=" + value + "\n")
		}
	}
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
