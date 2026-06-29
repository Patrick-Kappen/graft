package config

import "fmt"

func (f *File) Validate() error {
	if f.Version != 1 {
		return fmt.Errorf("unsupported or missing version %d", f.Version)
	}
	if f.Config.Runtime.Mode != "" && f.Config.Runtime.Mode != "rootfs-store" {
		return fmt.Errorf("unsupported runtime mode %q; expected rootfs-store", f.Config.Runtime.Mode)
	}
	if err := validateVolumes(f.Config.Filesystem.Volumes); err != nil {
		return err
	}
	if err := validateDevices(f.Config.Filesystem.Devices); err != nil {
		return err
	}
	if err := validateSecrets(f.Config.Secrets); err != nil {
		return err
	}
	return nil
}

func validateVolumes(volumes []VolumeConfig) error {
	seenTargets := map[string]struct{}{}
	for _, volume := range volumes {
		if volume.Source == "" && volume.Target == "" && volume.Mode == "" {
			continue
		}
		if volume.Source == "" || volume.Target == "" {
			return fmt.Errorf("filesystem volume must set both source and target")
		}
		if _, ok := seenTargets[volume.Target]; ok {
			return fmt.Errorf("duplicate filesystem volume target %q", volume.Target)
		}
		seenTargets[volume.Target] = struct{}{}
	}
	return nil
}

func validateDevices(devices []DeviceConfig) error {
	for _, device := range devices {
		if device.Source == "" && device.Target == "" && device.Permissions == "" {
			continue
		}
		if device.Source == "" {
			return fmt.Errorf("filesystem device must set source")
		}
	}
	return nil
}

func validateSecrets(secrets []SecretConfig) error {
	seenNames := map[string]struct{}{}
	for _, secret := range secrets {
		if secret.Name == "" {
			return fmt.Errorf("secret must set name")
		}
		if _, ok := seenNames[secret.Name]; ok {
			return fmt.Errorf("duplicate secret name %q", secret.Name)
		}
		seenNames[secret.Name] = struct{}{}
	}
	return nil
}

func (f *File) IsNoop() bool {
	cfg := f.Config
	return cfg.Runtime.Mode == "" &&
		len(cfg.Runtime.Packages) == 0 &&
		len(cfg.Runtime.PackageOps.Add) == 0 &&
		len(cfg.Runtime.PackageOps.Remove) == 0 &&
		len(cfg.Runtime.PackageOps.Replace) == 0 &&
		len(cfg.Runtime.Command) == 0 &&
		cfg.Container.Name == "" &&
		cfg.Container.Hostname == "" &&
		len(cfg.Container.Entrypoint) == 0 &&
		cfg.Container.StopSignal == "" &&
		cfg.Container.WorkingDir == "" &&
		cfg.Container.User == "" &&
		cfg.Container.Group == "" &&
		len(cfg.Container.Environment) == 0 &&
		len(cfg.Container.PodmanArgs) == 0 &&
		cfg.Filesystem.ReadOnly == nil &&
		cfg.Filesystem.ReadOnlyTmpfs == nil &&
		len(cfg.Filesystem.Tmpfs) == 0 &&
		len(cfg.Filesystem.Volumes) == 0 &&
		len(cfg.Filesystem.Mounts) == 0 &&
		len(cfg.Filesystem.Devices) == 0 &&
		cfg.Network.Mode == "" &&
		len(cfg.Network.Publish) == 0 &&
		len(cfg.Network.DNS) == 0 &&
		len(cfg.Network.AddHost) == 0 &&
		len(cfg.Security.DropCapabilities) == 0 &&
		len(cfg.Security.AddCapabilities) == 0 &&
		cfg.Security.NoNewPrivileges == nil &&
		cfg.Security.Privileged == nil &&
		cfg.Security.SeccompProfile == "" &&
		cfg.Security.SecurityLabelDisable == nil &&
		len(cfg.Security.SecurityOpt) == 0 &&
		cfg.Security.UserNS == "" &&
		cfg.Resources.Memory == "" &&
		cfg.Resources.MemorySwap == "" &&
		cfg.Resources.CPUs == "" &&
		cfg.Resources.CPUQuota == "" &&
		cfg.Resources.PidsLimit == 0 &&
		len(cfg.Resources.Ulimits) == 0 &&
		len(cfg.Secrets) == 0 &&
		cfg.Service.Type == "" &&
		cfg.Service.Restart == "" &&
		cfg.Service.RestartSec == "" &&
		cfg.Service.TimeoutStartSec == "" &&
		cfg.Service.TimeoutStopSec == "" &&
		cfg.Service.RemainAfterExit == nil &&
		len(cfg.Quadlet.Container) == 0 &&
		len(cfg.Quadlet.Service) == 0 &&
		len(cfg.Quadlet.Install) == 0
}
