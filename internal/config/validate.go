package config

import "fmt"

func (f *File) Validate() error {
	if f.Version != 1 {
		return fmt.Errorf("unsupported or missing version %d", f.Version)
	}
	if f.IsNoop() {
		return nil
	}
	if f.Config.Runtime.Mode != "rootfs-store" {
		return fmt.Errorf("unsupported runtime mode %q; expected rootfs-store", f.Config.Runtime.Mode)
	}
	if len(f.Config.Runtime.Command) == 0 {
		return fmt.Errorf("config.runtime.command must not be empty")
	}
	return nil
}

func (f *File) IsNoop() bool {
	cfg := f.Config
	return cfg.Runtime.Mode == "" &&
		len(cfg.Runtime.Packages) == 0 &&
		len(cfg.Runtime.Command) == 0 &&
		cfg.Container.Name == "" &&
		cfg.Container.WorkingDir == "" &&
		cfg.Container.User == "" &&
		cfg.Container.Group == "" &&
		len(cfg.Container.Environment) == 0 &&
		cfg.Filesystem.ReadOnly == nil &&
		cfg.Filesystem.ReadOnlyTmpfs == nil &&
		len(cfg.Filesystem.Tmpfs) == 0 &&
		len(cfg.Filesystem.Volumes) == 0 &&
		cfg.Network.Mode == "" &&
		len(cfg.Network.Publish) == 0 &&
		len(cfg.Security.DropCapabilities) == 0 &&
		len(cfg.Security.AddCapabilities) == 0 &&
		cfg.Security.NoNewPrivileges == nil &&
		cfg.Security.UserNS == "" &&
		cfg.Resources.Memory == "" &&
		cfg.Resources.PidsLimit == 0
}
