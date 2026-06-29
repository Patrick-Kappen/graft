package config

import "fmt"

func (f *File) Validate() error {
	if f.Version != 1 {
		return fmt.Errorf("unsupported or missing version %d", f.Version)
	}
	if f.Config.Runtime.Mode != "" && f.Config.Runtime.Mode != "rootfs-store" {
		return fmt.Errorf("unsupported runtime mode %q; expected rootfs-store", f.Config.Runtime.Mode)
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
